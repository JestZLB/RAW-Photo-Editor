import {
  useEffect,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
} from "react";
import type { MaskPoint } from "../api/types";
import { brushRadiusInFrame, sourcePointToView, viewPointToSource } from "../lib/mask";
import { useEditorStore } from "../stores/editorStore";

interface CursorState {
  x: number;
  y: number;
}

export function MaskBrushOverlay() {
  const doc = useEditorStore((state) => state.doc);
  const selectedLayerId = useEditorStore((state) => state.selectedLayerId);
  const active = useEditorStore((state) => state.maskBrushActive);
  const radius = useEditorStore((state) => state.maskBrushRadius);
  const erase = useEditorStore((state) => state.maskBrushErase);
  const addMaskStroke = useEditorStore((state) => state.addMaskStroke);

  const canvasRef = useRef<HTMLCanvasElement>(null);
  const dragRef = useRef<number | null>(null);
  const pointsRef = useRef<MaskPoint[]>([]);
  const [points, setPoints] = useState<MaskPoint[]>([]);
  const [cursor, setCursor] = useState<CursorState | null>(null);
  const [size, setSize] = useState({ width: 0, height: 0 });

  const layer = doc?.layers.find((item) => item.id === selectedLayerId) ?? null;
  const maskable = layer?.kind === "color" || layer?.kind === "portrait";
  const source = doc?.source;
  const crop = doc?.crop;

  // Same condition as the render guard below, so the observer is always
  // attached to the canvas that is actually on screen.
  const mounted = Boolean(active && maskable && doc && source && crop && layer);

  useEffect(() => {
    if (!mounted) return;
    const canvas = canvasRef.current;
    if (!canvas) return;
    const measure = (): void => {
      const rect = canvas.getBoundingClientRect();
      setSize((current) =>
        Math.abs(current.width - rect.width) < 1 && Math.abs(current.height - rect.height) < 1
          ? current
          : { width: rect.width, height: rect.height },
      );
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(canvas);
    return () => observer.disconnect();
  }, [mounted]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || !source || !crop || !layer || !active || !maskable) return;
    const width = Math.max(1, Math.round(size.width));
    const height = Math.max(1, Math.round(size.height));
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(height * dpr);
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, width, height);

    const drawStroke = (
      strokePoints: MaskPoint[],
      strokeRadius: number,
      isErase: boolean,
      opacity: number,
    ): void => {
      if (strokePoints.length === 0) return;
      const mapped = strokePoints.map((point) => {
        const view = sourcePointToView(point, crop, source.width, source.height, crop.angle ?? 0);
        return { x: view.x * width, y: view.y * height };
      });
      const lineWidth =
        brushRadiusInFrame(strokeRadius, crop, source.width, source.height, width) * 2;
      ctx.beginPath();
      if (mapped.length === 1) {
        ctx.arc(mapped[0].x, mapped[0].y, lineWidth / 2, 0, Math.PI * 2);
        ctx.fillStyle = isErase
          ? `rgba(255, 135, 147, ${opacity})`
          : `rgba(131, 181, 255, ${opacity})`;
        ctx.fill();
        return;
      }
      ctx.moveTo(mapped[0].x, mapped[0].y);
      for (let index = 1; index < mapped.length; index += 1) {
        ctx.lineTo(mapped[index].x, mapped[index].y);
      }
      ctx.lineWidth = lineWidth;
      ctx.lineCap = "round";
      ctx.lineJoin = "round";
      ctx.strokeStyle = isErase
        ? `rgba(255, 135, 147, ${opacity})`
        : `rgba(131, 181, 255, ${opacity})`;
      ctx.stroke();
    };

    for (const stroke of layer.mask?.strokes ?? []) {
      drawStroke(stroke.points, stroke.radius, stroke.erase, 0.18);
    }
    if (points.length > 0) {
      drawStroke(points, radius, erase, 0.38);
    }
  }, [active, crop, erase, layer, maskable, points, radius, size.height, size.width, source]);

  if (!mounted || !doc || !source || !crop || !layer) return null;

  const toSourcePoint = (event: ReactPointerEvent<HTMLCanvasElement>): MaskPoint => {
    const rect = event.currentTarget.getBoundingClientRect();
    const view = {
      x: (event.clientX - rect.left) / Math.max(1, rect.width),
      y: (event.clientY - rect.top) / Math.max(1, rect.height),
    };
    return viewPointToSource(view, crop, source.width, source.height, crop.angle ?? 0);
  };

  const updateCursor = (event: ReactPointerEvent<HTMLCanvasElement>): void => {
    const rect = event.currentTarget.getBoundingClientRect();
    setCursor({ x: event.clientX - rect.left, y: event.clientY - rect.top });
  };

  const beginStroke = (event: ReactPointerEvent<HTMLCanvasElement>): void => {
    if (event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    event.currentTarget.setPointerCapture(event.pointerId);
    dragRef.current = event.pointerId;
    pointsRef.current = [toSourcePoint(event)];
    setPoints([...pointsRef.current]);
    updateCursor(event);
  };

  const moveStroke = (event: ReactPointerEvent<HTMLCanvasElement>): void => {
    updateCursor(event);
    if (dragRef.current !== event.pointerId) return;
    const next = toSourcePoint(event);
    const last = pointsRef.current[pointsRef.current.length - 1];
    const rect = event.currentTarget.getBoundingClientRect();
    const threshold = 2 / Math.max(1, Math.max(rect.width, rect.height));
    if (
      last &&
      Math.abs(next.x - last.x) < threshold &&
      Math.abs(next.y - last.y) < threshold
    ) {
      return;
    }
    pointsRef.current = [...pointsRef.current, next];
    setPoints([...pointsRef.current]);
  };

  const endStroke = (event: ReactPointerEvent<HTMLCanvasElement>): void => {
    if (dragRef.current !== event.pointerId) return;
    dragRef.current = null;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    const completed = pointsRef.current;
    pointsRef.current = [];
    setPoints([]);
    if (completed.length > 0) {
      void addMaskStroke(layer.id, {
        points: completed,
        radius,
        feather: 0.35,
        erase,
      });
    }
  };

  const cursorRadius = brushRadiusInFrame(
    radius,
    crop,
    source.width,
    source.height,
    Math.max(1, size.width),
  );

  return (
    <>
      <canvas
        ref={canvasRef}
        className="mask-brush-canvas"
        aria-label="局部修饰画笔"
        onPointerDown={beginStroke}
        onPointerMove={moveStroke}
        onPointerUp={endStroke}
        onPointerCancel={endStroke}
        onPointerLeave={() => {
          if (dragRef.current === null) setCursor(null);
        }}
        onDoubleClick={(event) => event.stopPropagation()}
      />
      {cursor ? (
        <span
          className={`mask-brush-cursor ${erase ? "is-erase" : ""}`}
          style={{
            left: `${cursor.x}px`,
            top: `${cursor.y}px`,
            width: `${cursorRadius * 2}px`,
            height: `${cursorRadius * 2}px`,
          }}
          aria-hidden="true"
        />
      ) : null}
    </>
  );
}
