import { Check, Crop, RotateCcw, X } from "lucide-react";
import { cn } from "../../lib/cn";
import {
  CROP_ASPECT_OPTIONS,
  FULL_CROP,
  draftToRotatedPixels,
  rotatedFrameSize,
} from "../../lib/crop";
import { formatDimensions } from "../../lib/format";
import { useEditorStore } from "../../stores/editorStore";
import { PanelHeader } from "../../components/PanelHeader";
import { Button } from "../../components/controls/Button";
import { EmptyState } from "../../components/controls/EmptyState";
import { PanelSection } from "../../components/controls/PanelSection";
import { Slider } from "../../components/controls/Slider";

export function CropPanel() {
  const doc = useEditorStore((state) => state.doc);
  const mode = useEditorStore((state) => state.mode);
  const cropDraft = useEditorStore((state) => state.cropDraft);
  const cropAspect = useEditorStore((state) => state.cropAspect);
  const cropAngle = useEditorStore((state) => state.cropAngle);
  const setCropAngle = useEditorStore((state) => state.setCropAngle);
  const setCropAspect = useEditorStore((state) => state.setCropAspect);
  const setCropDraft = useEditorStore((state) => state.setCropDraft);
  const applyCrop = useEditorStore((state) => state.applyCrop);
  const resetCrop = useEditorStore((state) => state.resetCrop);
  const cancelCrop = useEditorStore((state) => state.cancelCrop);

  if (!doc?.source) {
    return (
      <div className="panel">
        <PanelHeader icon={<Crop size={18} />} title="裁剪" description="先打开一张照片" />
        <EmptyState
          icon={<Crop size={20} />}
          title="还没有照片"
          description="打开照片后可以在画布上框选裁剪区域。"
        />
      </div>
    );
  }

  const draft = cropDraft ?? FULL_CROP;
  const frame = rotatedFrameSize(doc.canvas.width, doc.canvas.height, cropAngle);
  const cropped = draftToRotatedPixels(draft, frame.width, frame.height);
  const cropWidth = cropped.width;
  const cropHeight = cropped.height;
  const isFullFrame =
    Math.abs(cropAngle) < 0.05 &&
    Math.abs(draft.x) < 0.0001 &&
    Math.abs(draft.y) < 0.0001 &&
    Math.abs(draft.width - 1) < 0.0001 &&
    Math.abs(draft.height - 1) < 0.0001;
  const isOriginal =
    Math.abs(doc.crop.angle ?? 0) < 0.001 &&
    doc.crop.x === 0 &&
    doc.crop.y === 0 &&
    doc.crop.width === doc.canvas.width &&
    doc.crop.height === doc.canvas.height;

  return (
    <div className="panel">
      <PanelHeader
        icon={<Crop size={18} />}
        title="裁剪"
        description="在画布上拖动边框，原图不会被修改"
      />

      <section className="panel-section">
        <div className="section-heading">
          <span>画面比例</span>
          <span className="section-note">点击套用</span>
        </div>
        <div className="crop-aspects" role="group" aria-label="裁剪比例">
          {CROP_ASPECT_OPTIONS.map((option) => (
            <button
              key={option.value}
              type="button"
              className={cn("crop-aspect", cropAspect === option.value && "is-active")}
              aria-pressed={cropAspect === option.value}
              onClick={() => setCropAspect(option.value)}
            >
              {option.label}
            </button>
          ))}
        </div>
        <p className="inline-note">
          选择比例会把裁剪框居中；拖动四角可继续调整，选中「自由」后不受比例限制。
        </p>
      </section>

      <section className="panel-section">
        <div className="section-heading">
          <span>角度</span>
          <span className="section-note">顺时针</span>
        </div>
        <Slider
          label="旋转"
          value={Number(cropAngle.toFixed(1))}
          min={-45}
          max={45}
          step={0.1}
          defaultValue={0}
          unit="°"
          compact
          disabled={mode !== "tauri"}
          formatValue={(value) => value.toFixed(1)}
          onChange={setCropAngle}
        />
        <p className="inline-note">
          {mode === "tauri"
            ? "角度只改变显示与输出，原图不动。旋转时裁剪框会自动内缩并保持在画面内，不会框到透明区域。"
            : "浏览器演示模式不执行角度旋转，请在桌面版使用。"}
        </p>
      </section>

      <PanelSection title="裁剪范围" hint={isFullFrame ? "整张照片" : "已框选"}>
        <div className="crop-facts">
          <div>
            <span>当前画面</span>
            <strong>{formatDimensions(doc.crop.width, doc.crop.height)}</strong>
          </div>
          <div>
            <span>裁剪后</span>
            <strong>{formatDimensions(cropWidth, cropHeight)}</strong>
          </div>
          <div>
            <span>原始照片</span>
            <strong>{formatDimensions(doc.canvas.width, doc.canvas.height)}</strong>
          </div>
        </div>
        <div className="crop-actions">
          <Button
            variant="primary"
            size="sm"
            icon={<Check size={14} />}
            disabled={isFullFrame}
            onClick={() => void applyCrop()}
          >
            应用裁剪
          </Button>
          <Button
            variant="ghost"
            size="sm"
            icon={<RotateCcw size={13} />}
            disabled={isOriginal}
            onClick={() => void resetCrop()}
          >
            恢复完整照片
          </Button>
        </div>
        <button
          type="button"
          className="panel-link crop-cancel"
          onClick={() => {
            setCropAspect("free");
            setCropDraft({ ...FULL_CROP });
          }}
        >
          <X size={11} aria-hidden="true" />
          重置当前选框
        </button>
        <button type="button" className="panel-link crop-cancel" onClick={cancelCrop}>
          <X size={11} aria-hidden="true" />
          退出裁剪
        </button>
      </PanelSection>

      <section className="panel-section panel-section--compact">
        <p className="field-hint">
          裁剪只在预览和导出时生效，原始文件与已有图层参数都会保留；应用后可用撤销恢复。
        </p>
      </section>
    </div>
  );
}
