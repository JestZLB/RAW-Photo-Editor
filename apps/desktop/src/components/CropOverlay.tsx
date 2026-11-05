import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import {
  aspectRatioOf,
  clampNormalizedCrop,
  type NormalizedCrop,
} from "../lib/crop";
import { useEditorStore } from "../stores/editorStore";

type DragMode = "move" | "nw" | "ne" | "sw" | "se";

interface DragState {
  mode: DragMode;
  pointerId: number;
  startX: number;
  startY: number;
  origin: NormalizedCrop;
  frameLeft: number;
  frameTop: number;
  frameWidth: number;
  frameHeight: number;
  ratio: number | null;
  capture: HTMLElement;
}

const HANDLE_MODES: DragMode[] = ["nw", "ne", "sw", "se"];

function resizeCrop(
  drag: DragState,
  pointerX: number,
  pointerY: number,
): NormalizedCrop {
  const origin = drag.origin;
  const anchorX = drag.mode.includes("w") ? origin.x + origin.width : origin.x;
  const anchorY = drag.mode.includes("n") ? origin.y + origin.height : origin.y;
  let width = Math.abs(pointerX - anchorX);
  let height = Math.abs(pointerY - anchorY);
  const min = 0.04;
  width = Math.min(1, Math.max(min, width));
  height = Math.min(1, Math.max(min, height));

  if (drag.ratio !== null && drag.frameWidth > 0 && drag.frameHeight > 0) {
    const normalizedRatio = (drag.frameWidth / drag.frameHeight) / drag.ratio;
    height = width * normalizedRatio;
    if (height > 1) {
      height = 1;
      width = height / normalizedRatio;
    }
    if (height < min) {
      height = min;
      width = height / normalizedRatio;
    }
  }

  const x = drag.mode.includes("w") ? anchorX - width : anchorX;
  const y = drag.mode.includes("n") ? anchorY - height : anchorY;
  return clampNormalizedCrop({ x, y, width, height });
}

export function CropOverlay() {
  const draft = useEditorStore((state) => state.cropDraft);
  const cropAspect = useEditorStore((state) => state.cropAspect);
  const setCropDraft = useEditorStore((state) => state.setCropDraft);
  const dragRef = useRef<DragState | null>(null);
  const [dragging, setDragging] = useState(false);

  useEffect(() => {
    const onPointerMove = (event: PointerEvent): void => {
      const drag = dragRef.current;
      if (!drag || drag.pointerId !== event.pointerId) return;
      if (drag.mode === "move") {
        const dx = (event.clientX - drag.startX) / drag.frameWidth;
        const dy = (event.clientY - drag.startY) / drag.frameHeight;
        setCropDraft(
          clampNormalizedCrop({
            x: drag.origin.x + dx,
            y: drag.origin.y + dy,
            width: drag.origin.width,
            height: drag.origin.height,
          }),
        );
        return;
      }
      const pointerX = (event.clientX - drag.frameLeft) / drag.frameWidth;
      const pointerY = (event.clientY - drag.frameTop) / drag.frameHeight;
      setCropDraft(resizeCrop(drag, pointerX, pointerY));
    };
    const onPointerUp = (event: PointerEvent): void => {
      const drag = dragRef.current;
      if (!drag || drag.pointerId !== event.pointerId) return;
      if (drag.capture.hasPointerCapture(event.pointerId)) {
        drag.capture.releasePointerCapture(event.pointerId);
      }
      dragRef.current = null;
      setDragging(false);
    };
    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", onPointerUp);
    window.addEventListener("pointercancel", onPointerUp);
    return () => {
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", onPointerUp);
      window.removeEventListener("pointercancel", onPointerUp);
    };
  }, [setCropDraft]);

  if (!draft) return null;

  const startDrag = (event: ReactPointerEvent<HTMLElement>, mode: DragMode): void => {
    if (event.button !== 0) return;
    const frame = event.currentTarget.closest<HTMLElement>(".crop-overlay");
    if (!frame) return;
    const rect = frame.getBoundingClientRect();
    event.preventDefault();
    event.stopPropagation();
    event.currentTarget.setPointerCapture(event.pointerId);
    dragRef.current = {
      mode,
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      origin: { ...draft },
      frameLeft: rect.left,
      frameTop: rect.top,
      frameWidth: Math.max(1, rect.width),
      frameHeight: Math.max(1, rect.height),
      ratio: aspectRatioOf(cropAspect),
      capture: event.currentTarget,
    };
    setDragging(true);
  };

  return (
    <div className={`crop-overlay ${dragging ? "is-dragging" : ""}`} aria-label="裁剪选区">
      <div
        className="crop-box"
        style={{
          left: `${draft.x * 100}%`,
          top: `${draft.y * 100}%`,
          width: `${draft.width * 100}%`,
          height: `${draft.height * 100}%`,
        }}
        onPointerDown={(event) => startDrag(event, "move")}
      >
        <span className="crop-grid" aria-hidden="true" />
        {HANDLE_MODES.map((mode) => (
          <span
            key={mode}
            className={`crop-handle crop-handle--${mode}`}
            role="presentation"
            onPointerDown={(event) => startDrag(event, mode)}
          />
        ))}
      </div>
    </div>
  );
}
