import { useEffect, useRef, useState } from "react";
import { faceScorePercent } from "../lib/face";
import { sourcePointToView } from "../lib/mask";
import { useEditorStore } from "../stores/editorStore";

interface Size {
  width: number;
  height: number;
}

/** Numbered face boxes so the multi-face list maps to the canvas. */
export function FaceBoxOverlay() {
  const doc = useEditorStore((state) => state.doc);
  const faceAnalysis = useEditorStore((state) => state.faceAnalysis);
  const ignoredFaceIds = useEditorStore((state) => state.ignoredFaceIds);
  const selectedFaceIndex = useEditorStore((state) => state.selectedFaceIndex);
  const rootRef = useRef<SVGSVGElement>(null);
  const [size, setSize] = useState<Size>({ width: 0, height: 0 });

  const source = doc?.source ?? null;
  const crop = doc?.crop ?? null;
  const faces = faceAnalysis?.faces ?? [];
  const visibleFaces = faces
    .map((face, index) => ({ face, index }))
    .filter(({ face }) => !ignoredFaceIds.includes(face.faceId));
  const visible = Boolean(source && crop && visibleFaces.length > 0);

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
  const ready = size.width > 0 && size.height > 0;

  const project = (x: number, y: number): { x: number; y: number } => {
    const view = sourcePointToView(
      { x: x / Math.max(1, source.width), y: y / Math.max(1, source.height) },
      crop,
      source.width,
      source.height,
      crop.angle ?? 0,
    );
    return { x: view.x * size.width, y: view.y * size.height };
  };

  const entries = visibleFaces.map(({ face, index }) => {
    const corners = [
      [face.x, face.y],
      [face.x + face.width, face.y],
      [face.x + face.width, face.y + face.height],
      [face.x, face.y + face.height],
    ].map(([x, y]) => project(x, y));
    return {
      face,
      index,
      points: corners.map((point) => `${point.x},${point.y}`).join(" "),
      left: Math.min(...corners.map((point) => point.x)),
      top: Math.min(...corners.map((point) => point.y)),
      selected: index === selectedFaceIndex,
    };
  });
  const placed: Array<{ left: number; top: number }> = [];
  const labelTops = new Map<number, number>();
  for (const entry of [...entries].sort(
    (a, b) => Number(b.selected) - Number(a.selected) || a.left - b.left,
  )) {
    let top = Math.max(14, entry.top + 4);
    while (
      placed.some(
        (item) => Math.abs(item.left - entry.left) < 86 && Math.abs(item.top - top) < 19,
      )
    ) {
      top += 19;
    }
    placed.push({ left: entry.left, top });
    labelTops.set(entry.index, top);
  }

  return (
    <svg
      ref={rootRef}
      className="face-box-overlay"
      aria-label="人脸检测位置"
      width="100%"
      height="100%"
    >
      {ready
        ? entries.map(({ face, index, points, left, selected }) => (
            <g key={face.faceId}>
              <polygon
                points={points}
                className={selected ? "face-box is-selected" : "face-box"}
              />
              <g
                transform={`translate(${Math.max(3, left + 4)}, ${labelTops.get(index) ?? 14})`}
              >
                <rect
                  className={selected ? "face-box__label is-selected" : "face-box__label"}
                  width={86}
                  height={17}
                  rx={4}
                />
                <text className="face-box__text" x={7} y={12}>
                  {`人脸 ${index + 1} · ${faceScorePercent(face.confidence)}%`}
                </text>
              </g>
            </g>
          ))
        : null}
    </svg>
  );
}
