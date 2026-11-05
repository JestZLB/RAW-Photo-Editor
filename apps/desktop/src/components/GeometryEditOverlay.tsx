import {
  useEffect,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
} from "react";
import type { PortraitEllipse, PortraitGeometry } from "../api/types";
import { portraitGeometryForFace } from "../lib/face";
import { sourcePointToView, viewPointToSource } from "../lib/mask";
import { useEditorStore } from "../stores/editorStore";

type GeometryKey = keyof PortraitGeometry;
type DragMode = "move" | "radiusX" | "radiusY";

interface DragState {
  key: GeometryKey;
  mode: DragMode;
  startSource: { x: number; y: number };
  start: PortraitEllipse;
}

const DRAW_ORDER: GeometryKey[] = [
  "neck",
  "face",
  "leftBrow",
  "rightBrow",
  "leftEye",
  "rightEye",
  "mouth",
  "lips",
  "teeth",
];

const LABELS: Record<GeometryKey, string> = {
  face: "脸型范围",
  leftEye: "左眼",
  rightEye: "右眼",
  leftBrow: "左眉",
  rightBrow: "右眉",
  mouth: "嘴部",
  lips: "嘴唇",
  teeth: "牙齿",
  neck: "颈部",
};

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

/**
 * Canvas editor for the measured ellipses. Dragging the shape moves it and
 * dragging the side handles changes its radii; everything is stored on the
 * selected face's portrait layer, so preview and export use the correction
 * without running detection again.
 */
export function GeometryEditOverlay() {
  const doc = useEditorStore((state) => state.doc);
  const tool = useEditorStore((state) => state.tool);
  const comparing = useEditorStore((state) => state.comparing);
  const splitCompare = useEditorStore((state) => state.splitCompare);
  const active = useEditorStore((state) => state.geometryEditActive);
  const maskBrushActive = useEditorStore((state) => state.maskBrushActive);
  const whiteBalancePickActive = useEditorStore((state) => state.whiteBalancePickActive);
  const faceAnalysis = useEditorStore((state) => state.faceAnalysis);
  const selectedFaceIndex = useEditorStore((state) => state.selectedFaceIndex);
  const faceRegions = useEditorStore((state) => state.faceRegions);
  const selectedLayerId = useEditorStore((state) => state.selectedLayerId);
  const updateFaceGeometry = useEditorStore((state) => state.updateFaceGeometry);
  const commitEdit = useEditorStore((state) => state.commitEdit);

  const rootRef = useRef<SVGSVGElement>(null);
  const dragRef = useRef<DragState | null>(null);
  const [size, setSize] = useState({ width: 0, height: 0 });
  const [selectedKey, setSelectedKey] = useState<GeometryKey>("face");

  const source = doc?.source ?? null;
  const crop = doc?.crop ?? null;
  const visible = Boolean(
    active &&
      tool === "portrait" &&
      !comparing &&
      !splitCompare &&
      !maskBrushActive &&
      !whiteBalancePickActive &&
      source &&
      crop,
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

  const face = faceAnalysis?.faces[selectedFaceIndex] ?? null;
  if (!face) return null;
  const layer =
    doc?.layers.find(
      (item) =>
        item.kind === "portrait" &&
        item.portrait?.faceIndex === selectedFaceIndex &&
        item.portrait?.bodyMode !== true,
    ) ??
    doc?.layers.find(
      (item) =>
        item.id === selectedLayerId &&
        item.kind === "portrait" &&
        item.portrait?.faceIndex == null &&
        item.portrait?.bodyMode !== true,
    ) ??
    null;
  const measured = faceRegions.find((item) => item.faceId === face.faceId) ?? null;
  const geometry =
    layer?.portrait?.geometry ??
    portraitGeometryForFace(face, source.width, source.height, measured);

  const ready = size.width > 0 && size.height > 0;
  const scaleX = size.width / Math.max(1, crop.width);
  const scaleY = size.height / Math.max(1, crop.height);

  const shapeOf = (ellipse: PortraitEllipse) => {
    const center = sourcePointToView(
      { x: ellipse.centerX, y: ellipse.centerY },
      crop,
      source.width,
      source.height,
      crop.angle ?? 0,
    );
    return {
      cx: center.x * size.width,
      cy: center.y * size.height,
      rx: Math.max(6, ellipse.radiusX * source.width * scaleX),
      ry: Math.max(6, ellipse.radiusY * source.height * scaleY),
    };
  };

  const pointerSource = (event: ReactPointerEvent<SVGElement>): { x: number; y: number } | null => {
    const root = rootRef.current;
    if (!root) return null;
    const rect = root.getBoundingClientRect();
    if (rect.width <= 0 || rect.height <= 0) return null;
    return viewPointToSource(
      { x: (event.clientX - rect.left) / rect.width, y: (event.clientY - rect.top) / rect.height },
      crop,
      source.width,
      source.height,
      crop.angle ?? 0,
    );
  };

  const beginDrag = (
    event: ReactPointerEvent<SVGElement>,
    key: GeometryKey,
    mode: DragMode,
  ): void => {
    const ellipse = geometry[key];
    const point = pointerSource(event);
    if (!ellipse || !point) return;
    event.preventDefault();
    event.stopPropagation();
    setSelectedKey(key);
    dragRef.current = {
      key,
      mode,
      startSource: point,
      start: { ...ellipse },
    };
    rootRef.current?.setPointerCapture(event.pointerId);
  };

  const moveDrag = (event: ReactPointerEvent<SVGSVGElement>): void => {
    const drag = dragRef.current;
    if (!drag) return;
    const point = pointerSource(event);
    if (!point) return;
    const current = geometry[drag.key];
    if (!current) return;
    let next: PortraitEllipse;
    if (drag.mode === "move") {
      next = {
        ...drag.start,
        centerX: clamp(drag.start.centerX + point.x - drag.startSource.x, 0, 1),
        centerY: clamp(drag.start.centerY + point.y - drag.startSource.y, 0, 1),
      };
    } else if (drag.mode === "radiusX") {
      next = {
        ...drag.start,
        radiusX: clamp(Math.abs(point.x - drag.start.centerX), 0.004, 0.9),
      };
    } else {
      next = {
        ...drag.start,
        radiusY: clamp(Math.abs(point.y - drag.start.centerY), 0.004, 0.9),
      };
    }
    updateFaceGeometry({ ...geometry, [drag.key]: next });
  };

  const endDrag = (): void => {
    if (!dragRef.current) return;
    dragRef.current = null;
    void commitEdit("调整五官位置");
  };

  const shapes = ready
    ? DRAW_ORDER.flatMap((key) => {
        const ellipse = geometry[key];
        if (!ellipse) return [];
        const shape = shapeOf(ellipse);
        const selected = selectedKey === key;
        const label = LABELS[key];
        return [
          <g key={key} className="geometry-overlay__group">
            <ellipse
              className="geometry-overlay__hit"
              cx={shape.cx}
              cy={shape.cy}
              rx={shape.rx}
              ry={shape.ry}
              onPointerDown={(event) => beginDrag(event, key, "move")}
            >
              <title>{`${label}：拖动移动，拖圆点调整范围`}</title>
            </ellipse>
            <ellipse
              className={`geometry-overlay__shape${selected ? " is-selected" : ""}`}
              cx={shape.cx}
              cy={shape.cy}
              rx={shape.rx}
              ry={shape.ry}
            />
            {selected ? (
              <>
                <circle
                  className="geometry-overlay__handle"
                  cx={shape.cx + shape.rx}
                  cy={shape.cy}
                  r={5}
                  onPointerDown={(event) => beginDrag(event, key, "radiusX")}
                >
                  <title>调整宽度</title>
                </circle>
                <circle
                  className="geometry-overlay__handle geometry-overlay__handle--y"
                  cx={shape.cx}
                  cy={shape.cy + shape.ry}
                  r={5}
                  onPointerDown={(event) => beginDrag(event, key, "radiusY")}
                >
                  <title>调整高度</title>
                </circle>
                <text
                  className="geometry-overlay__label"
                  x={shape.cx}
                  y={Math.max(12, shape.cy - shape.ry - 6)}
                  textAnchor="middle"
                >
                  {label}
                </text>
              </>
            ) : null}
          </g>,
        ];
      })
    : null;

  return (
    <svg
      ref={rootRef}
      className="geometry-overlay"
      aria-label="手动调整五官与脸型范围"
      width="100%"
      height="100%"
      onPointerMove={moveDrag}
      onPointerUp={endDrag}
      onPointerCancel={endDrag}
    >
      {shapes}
    </svg>
  );
}
