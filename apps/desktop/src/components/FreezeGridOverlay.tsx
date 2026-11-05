import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { useEditorStore } from "../stores/editorStore";
import { sourcePointToView, viewPointToSource } from "../lib/mask";

const COLS = 16;
const ROWS = 16;

/**
 * Coarse protection grid for liquify. Cells live on the full uncropped frame,
 * so a crop or straighten never moves the protected area relative to the body.
 */
export function FreezeGridOverlay() {
  const doc = useEditorStore((state) => state.doc);
  const tool = useEditorStore((state) => state.tool);
  const retouchMode = useEditorStore((state) => state.retouchMode);
  const comparing = useEditorStore((state) => state.comparing);
  const splitCompare = useEditorStore((state) => state.splitCompare);
  const maskBrushActive = useEditorStore((state) => state.maskBrushActive);
  const whiteBalancePickActive = useEditorStore((state) => state.whiteBalancePickActive);
  const toggleFreezeGridCell = useEditorStore((state) => state.toggleFreezeGridCell);
  const retouchLayer = useEditorStore(
    (state) => state.doc?.layers.find((item) => item.kind === "retouch") ?? null,
  );

  const rootRef = useRef<SVGSVGElement>(null);
  const [size, setSize] = useState({ width: 0, height: 0 });

  const source = doc?.source ?? null;
  const crop = doc?.crop ?? null;
  const visible = Boolean(
    tool === "retouch" &&
      retouchMode === "grid" &&
      source &&
      crop &&
      !comparing &&
      !splitCompare &&
      !maskBrushActive &&
      !whiteBalancePickActive,
  );

  useEffect(() => {
    if (!visible) return;
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
  }, [visible]);

  if (!visible || !source || !crop) return null;

  const grid = retouchLayer?.retouch?.freezeGrid ?? [];
  const project = (x: number, y: number): { x: number; y: number } => {
    const view = sourcePointToView({ x, y }, crop, source.width, source.height, crop.angle ?? 0);
    return { x: view.x * size.width, y: view.y * size.height };
  };

  const handleToggle = (event: ReactPointerEvent<SVGSVGElement>): void => {
    if (event.button !== 0) return;
    const rect = rootRef.current?.getBoundingClientRect();
    if (!rect || rect.width <= 0 || rect.height <= 0) return;
    const point = viewPointToSource(
      { x: (event.clientX - rect.left) / rect.width, y: (event.clientY - rect.top) / rect.height },
      crop,
      source.width,
      source.height,
      crop.angle ?? 0,
    );
    const column = Math.min(COLS - 1, Math.max(0, Math.floor(point.x * COLS)));
    const row = Math.min(ROWS - 1, Math.max(0, Math.floor(point.y * ROWS)));
    void toggleFreezeGridCell(row * COLS + column);
  };

  const lines = [];
  if (size.width > 0 && size.height > 0) {
    for (let index = 0; index <= COLS; index += 1) {
      const start = project(index / COLS, 0);
      const end = project(index / COLS, 1);
      lines.push(
        <line
          key={`v-${index}`}
          className="freeze-grid__line"
          x1={start.x}
          y1={start.y}
          x2={end.x}
          y2={end.y}
        />,
      );
    }
    for (let index = 0; index <= ROWS; index += 1) {
      const start = project(0, index / ROWS);
      const end = project(1, index / ROWS);
      lines.push(
        <line
          key={`h-${index}`}
          className="freeze-grid__line"
          x1={start.x}
          y1={start.y}
          x2={end.x}
          y2={end.y}
        />,
      );
    }
  }

  const cells = [];
  if (size.width > 0 && size.height > 0) {
    for (let row = 0; row < ROWS; row += 1) {
      for (let column = 0; column < COLS; column += 1) {
        if (!grid[row * COLS + column]) continue;
        const corners = [
          project(column / COLS, row / ROWS),
          project((column + 1) / COLS, row / ROWS),
          project((column + 1) / COLS, (row + 1) / ROWS),
          project(column / COLS, (row + 1) / ROWS),
        ];
        cells.push(
          <polygon
            key={`cell-${row}-${column}`}
            className="freeze-grid__cell"
            points={corners.map((corner) => `${corner.x},${corner.y}`).join(" ")}
          />,
        );
      }
    }
  }

  return (
    <svg
      ref={rootRef}
      className="freeze-grid"
      aria-label="液化网格保护"
      width="100%"
      height="100%"
      onPointerDown={handleToggle}
    >
      <rect width="100%" height="100%" fill="transparent" />
      {cells}
      {lines}
    </svg>
  );
}
