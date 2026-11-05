import { useEffect, useRef, useState } from "react";
import type { MaskRegion } from "../api/types";
import { sourcePointToView } from "../lib/mask";
import { useEditorStore } from "../stores/editorStore";

interface Size {
  width: number;
  height: number;
}

/**
 * Draws the detected facial regions over the canvas so the user can see what
 * the analyser actually measured before adding one to a layer mask.
 */
export function RegionMaskOverlay() {
  const doc = useEditorStore((state) => state.doc);
  const faceRegions = useEditorStore((state) => state.faceRegions);
  const faceAnalysis = useEditorStore((state) => state.faceAnalysis);
  const selectedFaceIndex = useEditorStore((state) => state.selectedFaceIndex);
  const overlayActive = useEditorStore((state) => state.regionOverlayActive);
  const maskBrushActive = useEditorStore((state) => state.maskBrushActive);
  const whiteBalancePickActive = useEditorStore((state) => state.whiteBalancePickActive);
  const splitCompare = useEditorStore((state) => state.splitCompare);
  const selectedLayerId = useEditorStore((state) => state.selectedLayerId);

  const rootRef = useRef<SVGSVGElement>(null);
  const [size, setSize] = useState<Size>({ width: 0, height: 0 });

  const source = doc?.source ?? null;
  const crop = doc?.crop ?? null;
  const selectedFaceId = faceAnalysis?.faces[selectedFaceIndex]?.faceId ?? null;
  const face =
    (selectedFaceId
      ? faceRegions.find((item) => item.faceId === selectedFaceId)
      : null) ??
    faceRegions[selectedFaceIndex] ??
    faceRegions[0] ??
    null;
  const maskRegions: MaskRegion[] =
    doc?.layers.find((layer) => layer.id === selectedLayerId)?.mask?.regions ?? [];
  const visible = Boolean(
    overlayActive &&
      face &&
      source &&
      crop &&
      !maskBrushActive &&
      !whiteBalancePickActive &&
      !splitCompare,
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

  if (!visible || !source || !crop || !face) {
    return null;
  }
  // The SVG has to be mounted before its box can be measured, so ellipses are
  // drawn on the next pass rather than blocking the first render.
  const ready = size.width > 0 && size.height > 0;

  const project = (x: number, y: number): { x: number; y: number } => {
    const view = sourcePointToView({ x, y }, crop, source.width, source.height, crop.angle ?? 0);
    return { x: view.x * size.width, y: view.y * size.height };
  };

  const ellipseOf = (region: {
    centerX: number;
    centerY: number;
    radiusX: number;
    radiusY: number;
  }) => {
    const center = project(region.centerX, region.centerY);
    return {
      cx: center.x,
      cy: center.y,
      rx: region.radiusX * source.width * (size.width / Math.max(1, crop.width)),
      ry: region.radiusY * source.height * (size.height / Math.max(1, crop.height)),
    };
  };

  const ellipses = face.regions.map((region) => {
    const ellipse = ellipseOf(region);
    const used = maskRegions.some(
      (item) =>
        item.kind === region.kind &&
        Math.abs(item.centerX - region.centerX) < 0.002 &&
        Math.abs(item.centerY - region.centerY) < 0.002,
    );
    return (
      <ellipse
        key={`${region.kind}-${region.centerX.toFixed(3)}-${region.centerY.toFixed(3)}`}
        cx={ellipse.cx}
        cy={ellipse.cy}
        rx={ellipse.rx}
        ry={ellipse.ry}
        className={cnRegion(used)}
        style={{ opacity: 0.35 + region.confidence * 0.6 }}
      >
        <title>{`${region.label} · 可信度 ${Math.round(region.confidence * 100)}%`}</title>
      </ellipse>
    );
  });

  const points = face.keypoints.map((keypoint) => {
    const point = project(keypoint.x, keypoint.y);
    return (
      <circle
        key={`${keypoint.kind}-${keypoint.x.toFixed(3)}`}
        cx={point.x}
        cy={point.y}
        r={2.4}
        className="region-overlay__point"
      >
        <title>{`${keypoint.label}`}</title>
      </circle>
    );
  });

  return (
    <svg
      ref={rootRef}
      className="region-overlay"
      aria-label="五官区域识别结果"
      role="img"
      width="100%"
      height="100%"
    >
      {ready ? ellipses : null}
      {ready ? points : null}
    </svg>
  );
}

function cnRegion(used: boolean): string {
  return used ? "region-overlay__region is-used" : "region-overlay__region";
}
