import { useCallback, useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { Eye, GripVertical, Image as ImageIcon, Maximize2, Minus, Plus } from "lucide-react";
import { resolveAssetUrl } from "../api/assets";
import { clamp } from "../lib/clamp";
import { sourcePlacement } from "../lib/compare";
import { formatDimensions } from "../lib/format";
import { rotatedFrameSize } from "../lib/crop";
import { useEditorStore } from "../stores/editorStore";
import { CropOverlay } from "./CropOverlay";
import { FaceBoxOverlay } from "./FaceBoxOverlay";
import { GradientMaskOverlay } from "./GradientMaskOverlay";
import { FreezeGridOverlay } from "./FreezeGridOverlay";
import { GeometryEditOverlay } from "./GeometryEditOverlay";
import { RegionMaskOverlay } from "./RegionMaskOverlay";
import { RetouchOverlay } from "./RetouchOverlay";
import { MaskBrushOverlay } from "./MaskBrushOverlay";
import { WhiteBalancePickerOverlay } from "./WhiteBalancePickerOverlay";
import { EmptyState } from "./controls/EmptyState";
import { Spinner } from "./controls/Spinner";

interface PanPoint {
  x: number;
  y: number;
}

interface DragState {
  pointerId: number;
  startX: number;
  startY: number;
  originX: number;
  originY: number;
}

const MIN_ZOOM = 0.5;
const MAX_ZOOM = 6;

export function CanvasStage() {
  const doc = useEditorStore((state) => state.doc);
  const preview = useEditorStore((state) => state.preview);
  const previewLoading = useEditorStore((state) => state.previewLoading);
  const comparing = useEditorStore((state) => state.comparing);
  const setComparing = useEditorStore((state) => state.setComparing);
  const splitCompare = useEditorStore((state) => state.splitCompare);
  const maskBrushActive = useEditorStore((state) => state.maskBrushActive);
  const whiteBalancePickActive = useEditorStore((state) => state.whiteBalancePickActive);
  const zoom = useEditorStore((state) => state.zoom);
  const setZoom = useEditorStore((state) => state.setZoom);
  const setFitPercent = useEditorStore((state) => state.setFitPercent);
  const tool = useEditorStore((state) => state.tool);
  const cropAngle = useEditorStore((state) => state.cropAngle);
  const mode = useEditorStore((state) => state.mode);
  const openPhoto = useEditorStore((state) => state.openPhoto);

  const viewportRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<DragState | null>(null);
  const [viewport, setViewport] = useState({ width: 0, height: 0 });
  const [pan, setPan] = useState<PanPoint>({ x: 0, y: 0 });
  const [dragging, setDragging] = useState(false);
  const [splitPercent, setSplitPercent] = useState(50);
  const splitDragRef = useRef<number | null>(null);
  const [splitDragging, setSplitDragging] = useState(false);

  const source = doc?.source ?? null;
  const imagePath = comparing ? source?.previewPath : preview?.path ?? source?.previewPath;
  const imageUrl = resolveAssetUrl(imagePath);
  const imageWidth = preview?.width ?? source?.width ?? 1;
  const imageHeight = preview?.height ?? source?.height ?? 1;
  const hasImage = Boolean(source && imageUrl);
  const sourceUrl = resolveAssetUrl(source?.previewPath);
  const isSplit =
    splitCompare &&
    !maskBrushActive &&
    !comparing &&
    tool !== "crop" &&
    Math.abs(doc?.crop.angle ?? 0) < 0.001 &&
    Boolean(sourceUrl && preview && doc && imageWidth > 0);

  const padX = 72;
  const padY = 108;
  const availableWidth = Math.max(120, viewport.width - padX);
  const availableHeight = Math.max(120, viewport.height - padY);
  // While cropping, the frame is the rotated bounding box and the image is
  // rotated inside it, so the crop handles stay upright.
  const cropping = tool === "crop" && !comparing;
  const frameSize = cropping
    ? rotatedFrameSize(imageWidth, imageHeight, cropAngle)
    : { width: imageWidth, height: imageHeight };
  const fitScale = hasImage
    ? Math.min(availableWidth / frameSize.width, availableHeight / frameSize.height)
    : 1;
  const displayScale = fitScale * zoom;
  const displayWidth = frameSize.width * displayScale;
  const displayHeight = frameSize.height * displayScale;
  const displayPercent = Math.max(1, Math.round(displayScale * 100));
  const canPan = hasImage && (displayWidth > availableWidth + 1 || displayHeight > availableHeight + 1);
  const splitPlacement =
    isSplit && source && doc
      ? sourcePlacement(source.width, source.height, doc.crop, displayWidth, displayHeight)
      : null;

  const viewRef = useRef({
    zoom,
    fitScale,
    pan,
    imageWidth,
    imageHeight,
    availableWidth,
    availableHeight,
    hasImage,
  });
  viewRef.current = { zoom, fitScale, pan, imageWidth, imageHeight, availableWidth, availableHeight, hasImage };

  const clampPan = useCallback(
    (point: PanPoint, width: number, height: number, availW: number, availH: number): PanPoint => {
      const maxX = Math.max(0, (width - availW) / 2);
      const maxY = Math.max(0, (height - availH) / 2);
      return { x: clamp(point.x, -maxX, maxX), y: clamp(point.y, -maxY, maxY) };
    },
    [],
  );

  useEffect(() => {
    const element = viewportRef.current;
    if (!element) return;
    const measure = (): void => {
      const rect = element.getBoundingClientRect();
      setViewport((current) =>
        Math.abs(current.width - rect.width) < 1 && Math.abs(current.height - rect.height) < 1
          ? current
          : { width: rect.width, height: rect.height },
      );
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    if (hasImage) setFitPercent(fitScale);
    else setFitPercent(0);
  }, [fitScale, hasImage, setFitPercent]);

  useEffect(() => {
    setPan({ x: 0, y: 0 });
    setDragging(false);
    setSplitPercent(50);
    dragRef.current = null;
    splitDragRef.current = null;
  }, [doc?.id]);

  useEffect(() => {
    setPan((current) => clampPan(current, displayWidth, displayHeight, availableWidth, availableHeight));
  }, [displayWidth, displayHeight, availableWidth, availableHeight, clampPan]);

  useEffect(() => {
    const element = viewportRef.current;
    if (!element) return;
    const onWheel = (event: WheelEvent): void => {
      event.preventDefault();
      const current = viewRef.current;
      if (!current.hasImage) return;
      const factor = event.deltaY < 0 ? 1.12 : 1 / 1.12;
      const nextZoom = clamp(current.zoom * factor, MIN_ZOOM, MAX_ZOOM);
      if (Math.abs(nextZoom - current.zoom) < 0.0001) return;
      const rect = element.getBoundingClientRect();
      const centerX = rect.width / 2;
      const centerY = rect.height / 2;
      const cursorX = event.clientX - rect.left - centerX;
      const cursorY = event.clientY - rect.top - centerY;
      const ratio = nextZoom / current.zoom;
      const nextPan = clampPan(
        {
          x: current.pan.x * ratio + cursorX * (1 - ratio),
          y: current.pan.y * ratio + cursorY * (1 - ratio),
        },
        current.imageWidth * current.fitScale * nextZoom,
        current.imageHeight * current.fitScale * nextZoom,
        current.availableWidth,
        current.availableHeight,
      );
      setZoom(nextZoom);
      setPan(nextPan);
    };
    element.addEventListener("wheel", onWheel, { passive: false });
    return () => element.removeEventListener("wheel", onWheel);
  }, [clampPan, setZoom]);

  const resetToFit = (): void => {
    setZoom(1);
    setPan({ x: 0, y: 0 });
  };

  const nudgeZoom = (factor: number): void => {
    const next = clamp(zoom * factor, MIN_ZOOM, MAX_ZOOM);
    setZoom(next);
    if (next <= 1.001) setPan({ x: 0, y: 0 });
  };

  const toggleNatural = (): void => {
    if (!hasImage) return;
    if (zoom > 1.05) {
      resetToFit();
      return;
    }
    const naturalZoom = clamp(1 / fitScale, MIN_ZOOM, MAX_ZOOM);
    setZoom(naturalZoom);
    setPan({ x: 0, y: 0 });
  };

  const onPointerDown = (event: ReactPointerEvent<HTMLDivElement>): void => {
    if (event.button !== 0 || !canPan) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    dragRef.current = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      originX: pan.x,
      originY: pan.y,
    };
    setDragging(true);
  };

  const onPointerMove = (event: ReactPointerEvent<HTMLDivElement>): void => {
    const drag = dragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    setPan(
      clampPan(
        {
          x: drag.originX + (event.clientX - drag.startX),
          y: drag.originY + (event.clientY - drag.startY),
        },
        displayWidth,
        displayHeight,
        availableWidth,
        availableHeight,
      ),
    );
  };

  const endDrag = (event: ReactPointerEvent<HTMLDivElement>): void => {
    if (dragRef.current?.pointerId !== event.pointerId) return;
    dragRef.current = null;
    setDragging(false);
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  };

  const startCompare = (event: ReactPointerEvent<HTMLButtonElement>): void => {
    if (!source) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    setComparing(true);
  };

  const beginSplitDrag = (event: ReactPointerEvent<HTMLDivElement>): void => {
    event.preventDefault();
    event.stopPropagation();
    event.currentTarget.setPointerCapture(event.pointerId);
    splitDragRef.current = event.pointerId;
    setSplitDragging(true);
  };

  const moveSplitDrag = (event: ReactPointerEvent<HTMLDivElement>): void => {
    if (splitDragRef.current !== event.pointerId) return;
    const frame = event.currentTarget.parentElement;
    if (!frame) return;
    const rect = frame.getBoundingClientRect();
    if (rect.width <= 0) return;
    setSplitPercent(clamp(((event.clientX - rect.left) / rect.width) * 100, 5, 95));
  };

  const endSplitDrag = (event: ReactPointerEvent<HTMLDivElement>): void => {
    if (splitDragRef.current !== event.pointerId) return;
    splitDragRef.current = null;
    setSplitDragging(false);
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  };

  const modeLabel =
    mode === "demo"
      ? "演示模式 · 预览为本地近似效果"
      : preview?.backend === "wgpu"
        ? "GPU 渲染"
        : preview?.backend === "cache"
          ? "预览缓存"
          : preview?.backend === "source"
            ? "源图预览"
            : "CPU 渲染";
  const compareDimensions = comparing && source ? formatDimensions(source.width, source.height) : formatDimensions(imageWidth, imageHeight);

  return (
    <section className="canvas-stage" aria-label="照片画布">
      <div className="canvas-atmosphere" aria-hidden="true" />

      <div
        ref={viewportRef}
        className={`canvas-viewport ${dragging ? "is-dragging" : ""} ${canPan ? "is-pannable" : ""}`}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
        onDoubleClick={toggleNatural}
      >
        {hasImage ? (
          <div
            className={`canvas-frame ${comparing ? "is-comparing" : ""}`}
            style={{
              width: `${displayWidth}px`,
              height: `${displayHeight}px`,
              transform: `translate3d(${pan.x}px, ${pan.y}px, 0)`,
            }}
          >
            <img
              src={imageUrl ?? ""}
              alt={comparing ? "原始照片预览" : "照片预览"}
              draggable={false}
              onDragStart={(event) => event.preventDefault()}
              style={
                cropping
                  ? {
                      position: "absolute",
                      left: "50%",
                      top: "50%",
                      width: `${imageWidth * displayScale}px`,
                      height: `${imageHeight * displayScale}px`,
                      transform: `translate(-50%, -50%) rotate(${cropAngle}deg)`,
                    }
                  : undefined
              }
            />
            {isSplit && sourceUrl && splitPlacement ? (
              <>
                <div className="split-original" style={{ width: `${splitPercent}%` }} aria-hidden="true">
                  <div
                    className="split-original__surface"
                    style={{ width: `${displayWidth}px`, height: `${displayHeight}px` }}
                  >
                    <img
                      src={sourceUrl}
                      alt=""
                      draggable={false}
                      onDragStart={(event) => event.preventDefault()}
                      style={{
                        width: `${splitPlacement.width}px`,
                        height: `${splitPlacement.height}px`,
                        left: `${splitPlacement.left}px`,
                        top: `${splitPlacement.top}px`,
                      }}
                    />
                  </div>
                </div>
                <span className="split-tag split-tag--left">原图</span>
                <span className="split-tag split-tag--right">效果</span>
                <div
                  className={`split-divider ${splitDragging ? "is-dragging" : ""}`}
                  style={{ left: `${splitPercent}%` }}
                  role="slider"
                  tabIndex={0}
                  aria-label="分屏对比位置"
                  aria-valuemin={5}
                  aria-valuemax={95}
                  aria-valuenow={Math.round(splitPercent)}
                  title="拖动比较原图与当前效果"
                  onPointerDown={beginSplitDrag}
                  onPointerMove={moveSplitDrag}
                  onPointerUp={endSplitDrag}
                  onPointerCancel={endSplitDrag}
                  onDoubleClick={(event) => event.stopPropagation()}
                  onKeyDown={(event) => {
                    if (event.key === "ArrowLeft") {
                      event.preventDefault();
                      setSplitPercent((value) => clamp(value - 2, 5, 95));
                    } else if (event.key === "ArrowRight") {
                      event.preventDefault();
                      setSplitPercent((value) => clamp(value + 2, 5, 95));
                    } else if (event.key === "Home") {
                      event.preventDefault();
                      setSplitPercent(5);
                    } else if (event.key === "End") {
                      event.preventDefault();
                      setSplitPercent(95);
                    }
                  }}
                >
                  <span className="split-divider__handle">
                    <GripVertical size={12} aria-hidden="true" />
                  </span>
                </div>
              </>
            ) : null}
            {tool === "portrait" &&
            !comparing &&
            !splitCompare &&
            !maskBrushActive &&
            !whiteBalancePickActive ? (
              <FaceBoxOverlay />
            ) : null}
            {!comparing && tool !== "crop" ? <GradientMaskOverlay /> : null}
            {!comparing && tool !== "crop" ? <RegionMaskOverlay /> : null}
            {!comparing && tool !== "crop" ? <GeometryEditOverlay /> : null}
            {!comparing && tool !== "crop" ? <RetouchOverlay /> : null}
            {!comparing && tool !== "crop" ? <FreezeGridOverlay /> : null}
            {maskBrushActive && !comparing && tool !== "crop" ? <MaskBrushOverlay /> : null}
            {whiteBalancePickActive && !comparing && tool !== "crop" ? (
              <WhiteBalancePickerOverlay />
            ) : null}
            {tool === "crop" && !comparing ? <CropOverlay /> : null}
            {comparing ? (
              <span className="canvas-frame__badge">
                <Eye size={11} aria-hidden="true" />
                原图
              </span>
            ) : null}
          </div>
        ) : (
          <div className="canvas-empty" onPointerDown={(event) => event.stopPropagation()}>
            <EmptyState
              icon={<ImageIcon size={22} />}
              title="这个项目还没有照片"
              description="打开一张人像照片，所有编辑都会以新图层保存。"
              action={
                <button type="button" className="btn btn--primary btn--sm" onClick={() => void openPhoto()}>
                  打开照片
                </button>
              }
            />
          </div>
        )}
      </div>

      {hasImage ? (
        <>
          <div className="canvas-chip" role="status">
            <span className={`status-dot ${mode === "demo" ? "status-dot--amber" : "status-dot--green"}`} aria-hidden="true" />
            <span className="canvas-chip__dims">{compareDimensions}</span>
            <span className="canvas-chip__sep" aria-hidden="true" />
            <span className="canvas-chip__mode" title={mode === "demo" ? "浏览器演示使用本地 canvas 近似渲染" : "预览由本地引擎渲染"}>
              {modeLabel}
            </span>
          </div>

          <button
            type="button"
            className={`compare-pill ${comparing ? "is-active" : ""}`}
            aria-label="按住查看原图"
            aria-pressed={comparing}
            title="按住查看未修改的原图"
            onPointerDown={startCompare}
            onPointerUp={() => setComparing(false)}
            onPointerCancel={() => setComparing(false)}
            onPointerLeave={() => setComparing(false)}
            onKeyDown={(event) => {
              if (event.key === " " || event.key === "Enter") {
                event.preventDefault();
                setComparing(true);
              }
            }}
            onKeyUp={(event) => {
              if (event.key === " " || event.key === "Enter") {
                event.preventDefault();
                setComparing(false);
              }
            }}
            onBlur={() => setComparing(false)}
          >
            <Eye size={13} aria-hidden="true" />
            按住看原图
          </button>

          {previewLoading && !comparing ? (
            <div className="canvas-loading" role="status" aria-live="polite">
              <Spinner size={12} />
              <span>正在更新预览…</span>
            </div>
          ) : null}

          <div className="zoom-dock" aria-label="缩放">
            <button
              type="button"
              className="zoom-dock__btn"
              aria-label="缩小"
              title="缩小（-）"
              disabled={zoom <= MIN_ZOOM + 0.001}
              onClick={() => nudgeZoom(1 / 1.25)}
            >
              <Minus size={13} />
            </button>
            <button
              type="button"
              className="zoom-dock__value"
              title="适应窗口（双击画布可在适应与 100% 之间切换）"
              onClick={resetToFit}
            >
              {displayPercent}%
            </button>
            <button
              type="button"
              className="zoom-dock__btn"
              aria-label="放大"
              title="放大（+）"
              disabled={zoom >= MAX_ZOOM - 0.001}
              onClick={() => nudgeZoom(1.25)}
            >
              <Plus size={13} />
            </button>
            <span className="zoom-dock__sep" aria-hidden="true" />
            <button type="button" className="zoom-dock__btn" aria-label="适应窗口" title="适应窗口（0）" onClick={resetToFit}>
              <Maximize2 size={13} />
            </button>
          </div>
        </>
      ) : null}
    </section>
  );
}
