import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import type { MaskPoint } from "../api/types";
import { sourcePointToView, viewPointToSource } from "../lib/mask";
import { useEditorStore } from "../stores/editorStore";
import { toastError } from "../stores/toastStore";

interface Size {
  width: number;
  height: number;
}

/**
 * Stamps clone/heal spots on the canvas. The circle follows the pointer and a
 * second marker shows the clone sample point.
 */
export function RetouchOverlay() {
  const doc = useEditorStore((state) => state.doc);
  const tool = useEditorStore((state) => state.tool);
  const comparing = useEditorStore((state) => state.comparing);
  const splitCompare = useEditorStore((state) => state.splitCompare);
  const maskBrushActive = useEditorStore((state) => state.maskBrushActive);
  const whiteBalancePickActive = useEditorStore((state) => state.whiteBalancePickActive);
  const mode = useEditorStore((state) => state.retouchMode);
  const radius = useEditorStore((state) => state.retouchRadius);
  const strength = useEditorStore((state) => state.retouchStrength);
  const source = useEditorStore((state) => state.retouchSource);
  const setSource = useEditorStore((state) => state.setRetouchSource);
  const addSpot = useEditorStore((state) => state.addRetouchSpot);
  const addLiquifyStroke = useEditorStore((state) => state.addLiquifyStroke);
  const addFreezeStroke = useEditorStore((state) => state.addFreezeStroke);

  const rootRef = useRef<SVGSVGElement>(null);
  const drawing = useRef<MaskPoint[] | null>(null);
  const [size, setSize] = useState<Size>({ width: 0, height: 0 });
  const [hover, setHover] = useState<{ x: number; y: number } | null>(null);

  const active = Boolean(
    tool === "retouch" &&
      mode !== "grid" &&
      doc?.source &&
      doc?.crop &&
      !comparing &&
      !splitCompare &&
      !maskBrushActive &&
      !whiteBalancePickActive,
  );

  useEffect(() => {
    if (!active) return;
    const element = rootRef.current;
    if (!element) return;
    const measure = (): void => {
      const rect = element.getBoundingClientRect();
      setSize((current) =>
        Math.abs(current.width - rect.width) < 0.5 &&
        Math.abs(current.height - rect.height) < 0.5
          ? current
          : { width: rect.width, height: rect.height },
      );
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, [active]);

  if (!active || !doc || !doc.source || !doc.crop) return null;
  const sourceWidth = doc.source.width;
  const sourceHeight = doc.source.height;
  const crop = doc.crop;
  const ready = size.width > 0 && size.height > 0;
  const viewRadius = (): number =>
    radius * Math.min(sourceWidth, sourceHeight) * (size.width / Math.max(1, crop.width));
  const project = (x: number, y: number): { x: number; y: number } => {
    const view = sourcePointToView({ x, y }, crop, sourceWidth, sourceHeight, crop.angle ?? 0);
    return { x: view.x * size.width, y: view.y * size.height };
  };

  const toPoint = (event: ReactPointerEvent<SVGSVGElement>) => {
    const rect = event.currentTarget.getBoundingClientRect();
    const view = {
      x: (event.clientX - rect.left) / Math.max(1, rect.width),
      y: (event.clientY - rect.top) / Math.max(1, rect.height),
    };
    return {
      view,
      source: viewPointToSource(view, crop, sourceWidth, sourceHeight, crop.angle ?? 0),
    };
  };

  const handlePointerDown = (event: ReactPointerEvent<SVGSVGElement>): void => {
    if (event.button !== 0 || mode === "grid") return;
    event.preventDefault();
    event.stopPropagation();
    const { view, source: point } = toPoint(event);
    if (view.x < 0 || view.x > 1 || view.y < 0 || view.y > 1) return;
    if (
      mode === "push" ||
      mode === "pinch" ||
      mode === "bulge" ||
      mode === "freeze"
    ) {
      drawing.current = [point];
      event.currentTarget.setPointerCapture(event.pointerId);
      return;
    }
    if (mode === "clone" && event.altKey) {
      setSource(point);
      return;
    }
    if (mode === "clone" && !source) {
      toastError("先按住 Alt 点击一处作为取样点。", "还没有取样点");
      return;
    }
    void addSpot({
      mode,
      x: point.x,
      y: point.y,
      radius,
      sourceX: mode === "clone" ? (source?.x ?? null) : null,
      sourceY: mode === "clone" ? (source?.y ?? null) : null,
      hardness: 0.4,
      opacity: 1,
    });
  };

  const handlePointerMove = (event: ReactPointerEvent<SVGSVGElement>): void => {
    const { view, source: point } = toPoint(event);
    setHover(view.x < 0 || view.x > 1 || view.y < 0 || view.y > 1 ? null : point);
    const points = drawing.current;
    if (!points || points.length >= 512) return;
    if (view.x < 0 || view.x > 1 || view.y < 0 || view.y > 1) return;
    const last = points[points.length - 1];
    const step = Math.hypot(point.x - last.x, point.y - last.y);
    if (step > radius * 0.12) points.push(point);
  };

  const finishLiquify = (event: ReactPointerEvent<SVGSVGElement>): void => {
    const points = drawing.current;
    drawing.current = null;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    if (!points || points.length === 0) return;
    if (mode === "freeze") {
      void addFreezeStroke({ points, radius, feather: 0.5, erase: false });
      return;
    }
    if (mode !== "push" && mode !== "pinch" && mode !== "bulge") return;
    if (mode === "push" && points.length < 2) return;
    void addLiquifyStroke({ points, radius, strength, mode });
  };

  const hoverPoint = hover ? project(hover.x, hover.y) : null;
  const sourcePoint = source ? project(source.x, source.y) : null;

  return (
    <svg
      ref={rootRef}
      className="retouch-overlay"
      aria-label="局部修复取样与笔刷"
      role="img"
      width="100%"
      height="100%"
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
      onPointerUp={finishLiquify}
      onPointerCancel={finishLiquify}
      onPointerLeave={() => setHover(null)}
      onContextMenu={(event) => event.preventDefault()}
    >
      {ready && hoverPoint ? (
        <circle
          className={
            mode === "freeze"
              ? "retouch-overlay__brush is-freeze"
              : "retouch-overlay__brush"
          }
          cx={hoverPoint.x}
          cy={hoverPoint.y}
          r={Math.max(3, viewRadius())}
        />
      ) : null}
      {ready && sourcePoint ? (
        <g className="retouch-overlay__source">
          <circle cx={sourcePoint.x} cy={sourcePoint.y} r={Math.max(3, viewRadius())} />
          <circle cx={sourcePoint.x} cy={sourcePoint.y} r={2.5} />
        </g>
      ) : null}
    </svg>
  );
}
