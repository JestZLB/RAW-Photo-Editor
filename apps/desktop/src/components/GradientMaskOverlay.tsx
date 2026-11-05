import {
  useEffect,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
} from "react";
import type { MaskGradient, MaskPoint } from "../api/types";
import { clamp01 } from "../lib/curve";
import { viewPointToSource, sourcePointToView } from "../lib/mask";
import { useEditorStore } from "../stores/editorStore";

type HandleId = "start" | "end";

interface Size {
  width: number;
  height: number;
}

function viewPoint(point: MaskPoint, size: Size): { x: number; y: number } {
  return { x: point.x * size.width, y: point.y * size.height };
}

export function GradientMaskOverlay() {
  const doc = useEditorStore((state) => state.doc);
  const selectedLayerId = useEditorStore((state) => state.selectedLayerId);
  const maskBrushActive = useEditorStore((state) => state.maskBrushActive);
  const whiteBalancePickActive = useEditorStore((state) => state.whiteBalancePickActive);
  const splitCompare = useEditorStore((state) => state.splitCompare);
  const setMaskGradient = useEditorStore((state) => state.setMaskGradient);

  const layer = doc?.layers.find((item) => item.id === selectedLayerId) ?? null;
  const gradient = layer?.mask?.gradient ?? null;
  const source = doc?.source;
  const crop = doc?.crop;

  const rootRef = useRef<SVGSVGElement>(null);
  const [size, setSize] = useState<Size>({ width: 0, height: 0 });
  const [draft, setDraft] = useState<MaskGradient | null>(gradient);
  const draftRef = useRef<MaskGradient | null>(gradient);
  const handleRef = useRef<HandleId | null>(null);
  const pointerRef = useRef<number | null>(null);
  const captureRef = useRef<Element | null>(null);

  // Must match the render guard below: if the SVG unmounts while this stays
  // true, the observer keeps watching a detached node and the size freezes.
  const visible = Boolean(
    gradient && !maskBrushActive && !whiteBalancePickActive && !splitCompare,
  );

  useEffect(() => {
    if (!visible) return;
    const element = rootRef.current;
    if (!element) return;
    const measure = (): void => {
      const rect = element.getBoundingClientRect();
      setSize((current) =>
        Math.abs(current.width - rect.width) < 0.5 && Math.abs(current.height - rect.height) < 0.5
          ? current
          : { width: rect.width, height: rect.height },
      );
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, [visible]);

  useEffect(() => {
    setDraft(gradient);
    draftRef.current = gradient;
  }, [gradient]);

  if (
    !doc ||
    !source ||
    !crop ||
    !layer ||
    !gradient ||
    maskBrushActive ||
    whiteBalancePickActive ||
    splitCompare
  ) {
    return null;
  }

  const current = draft ?? gradient;
  const startView = sourcePointToView(current.start, crop, source.width, source.height, crop.angle ?? 0);
  const endView = sourcePointToView(current.end, crop, source.width, source.height, crop.angle ?? 0);
  const start = viewPoint(
    { x: clamp01(startView.x), y: clamp01(startView.y) },
    size,
  );
  const end = viewPoint({ x: clamp01(endView.x), y: clamp01(endView.y) }, size);
  const radius = Math.hypot(end.x - start.x, end.y - start.y);

  const beginDrag = (
    handle: HandleId,
    event: ReactPointerEvent<SVGCircleElement>,
  ): void => {
    event.preventDefault();
    event.stopPropagation();
    handleRef.current = handle;
    pointerRef.current = event.pointerId;
    captureRef.current = event.currentTarget;
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const moveDrag = (event: ReactPointerEvent<SVGSVGElement>): void => {
    if (handleRef.current === null || pointerRef.current !== event.pointerId) return;
    const rect = rootRef.current?.getBoundingClientRect();
    if (!rect || rect.width <= 0 || rect.height <= 0) return;
    const sourcePoint = viewPointToSource(
      {
        x: clamp01((event.clientX - rect.left) / rect.width),
        y: clamp01((event.clientY - rect.top) / rect.height),
      },
      crop,
      source.width,
      source.height,
      crop.angle ?? 0,
    );
    const base = draftRef.current ?? gradient;
    const next: MaskGradient = {
      ...base,
      [handleRef.current]: sourcePoint,
    };
    const dx = (next.end.x - next.start.x) * source.width;
    const dy = (next.end.y - next.start.y) * source.height;
    if (Math.hypot(dx, dy) < 6) return;
    draftRef.current = next;
    setDraft(next);
  };

  const endDrag = (event: ReactPointerEvent<SVGSVGElement>): void => {
    if (pointerRef.current !== event.pointerId) return;
    const capture = captureRef.current;
    if (capture instanceof Element && capture.hasPointerCapture(event.pointerId)) {
      capture.releasePointerCapture(event.pointerId);
    }
    handleRef.current = null;
    pointerRef.current = null;
    captureRef.current = null;
    const next = draftRef.current;
    if (
      next &&
      (next.start.x !== gradient.start.x ||
        next.start.y !== gradient.start.y ||
        next.end.x !== gradient.end.x ||
        next.end.y !== gradient.end.y)
    ) {
      void setMaskGradient(layer.id, next);
    }
  };

  const handle = (id: HandleId, point: { x: number; y: number }): ReactNode => (
    <g key={id}>
      <circle
        className="gradient-mask-overlay__handle"
        cx={point.x}
        cy={point.y}
        r={6}
      />
      <circle
        className="gradient-mask-overlay__hit"
        cx={point.x}
        cy={point.y}
        r={15}
        onPointerDown={(event) => beginDrag(id, event)}
      />
    </g>
  );

  return (
    <svg
      ref={rootRef}
      className="gradient-mask-overlay"
      viewBox={`0 0 ${Math.max(1, size.width)} ${Math.max(1, size.height)}`}
      aria-label="拖动端点调整渐变修饰范围"
      onPointerMove={moveDrag}
      onPointerUp={endDrag}
      onPointerCancel={endDrag}
      onDoubleClick={(event) => event.stopPropagation()}
    >
      {current.kind === "linear" ? (
        <line
          className="gradient-mask-overlay__guide"
          x1={start.x}
          y1={start.y}
          x2={end.x}
          y2={end.y}
        />
      ) : (
        <>
          <circle
            className="gradient-mask-overlay__guide"
            cx={start.x}
            cy={start.y}
            r={radius}
          />
          <line
            className="gradient-mask-overlay__guide gradient-mask-overlay__guide--muted"
            x1={start.x}
            y1={start.y}
            x2={end.x}
            y2={end.y}
          />
        </>
      )}
      {handle("start", start)}
      {handle("end", end)}
    </svg>
  );
}
