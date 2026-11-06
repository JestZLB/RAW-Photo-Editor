import { useEffect, useId, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from "react";
import { createPortal } from "react-dom";
import { AlertTriangle, Download, FileImage, ImagePlus, LockKeyhole, Trash2, X } from "lucide-react";
import type {
  ExportBitDepth,
  ExportColorSpace,
  ExportFormat,
  ExportMetadata,
} from "../api/types";
import { isApiError, userMessageOf } from "../api/errors";
import { cn } from "../lib/cn";
import { pickFile, readAsDataUrl } from "../lib/file";
import {
  EXPORT_SIZE_OPTIONS,
  extensionForFormat,
  outputDimensions as computeOutputDimensions,
  renderExportStem,
  type ExportSize,
} from "../lib/export";
import { formatDimensions } from "../lib/format";
import { buildWatermarkOptions } from "../lib/watermark";
import { useEditorStore } from "../stores/editorStore";
import {
  MAX_WATERMARK_DATA_URL,
  WATERMARK_KINDS,
  WATERMARK_POSITIONS,
  useSettingsStore,
  type WatermarkKind,
  type WatermarkPositionId,
} from "../stores/settingsStore";
import { toastSuccess } from "../stores/toastStore";
import { Button } from "./controls/Button";
import { DimensionInput } from "./controls/DimensionInput";
import { Segmented } from "./controls/Segmented";
import { Slider } from "./controls/Slider";
import { Toggle } from "./controls/Toggle";

const FORMATS: Array<{ value: ExportFormat; label: string; description: string }> = [
  { value: "jpeg", label: "JPEG", description: "体积小，适合分享" },
  { value: "png", label: "PNG", description: "无损，适合存档" },
  { value: "tiff", label: "TIFF", description: "印刷级，桌面版支持" },
  { value: "webp", label: "WebP", description: "无损小体积，适合网页" },
];

const MAX_OUTPUT_PIXELS = 100_000_000;

const COLOR_SPACES: Array<{
  value: ExportColorSpace;
  label: string;
  description: string;
}> = [
  { value: "srgb", label: "sRGB", description: "通用兼容，分享与网页" },
  { value: "displayP3", label: "Display P3", description: "广色域，需支持的设备观看" },
  { value: "source", label: "原图配置", description: "按照片自带的 ICC 配置导出" },
];

const METADATA_MODES: Array<{
  value: ExportMetadata;
  label: string;
  description: string;
}> = [
  { value: "keep", label: "保留", description: "机型与拍摄参数，去掉方向与旧缩略图" },
  { value: "privacy", label: "去除位置", description: "保留拍摄参数，删除 GPS 位置" },
  { value: "none", label: "不写入", description: "干净文件，适合公开发布" },
];

const WATERMARK_POSITION_LABELS: Record<WatermarkPositionId, string> = {
  topLeft: "左上",
  topRight: "右上",
  bottomLeft: "左下",
  bottomRight: "右下",
  center: "居中",
};

const WATERMARK_KIND_OPTIONS = WATERMARK_KINDS.map((kind) => ({
  value: kind,
  label: kind === "text" ? "文字" : "Logo",
}));

const WATERMARK_POSITION_OPTIONS = WATERMARK_POSITIONS.map((position) => ({
  value: position,
  label: WATERMARK_POSITION_LABELS[position],
}));

export function ExportDialog() {
  const open = useEditorStore((state) => state.exportOpen);
  const close = useEditorStore((state) => state.closeExport);
  const doc = useEditorStore((state) => state.doc);
  const mode = useEditorStore((state) => state.mode);
  const chooseExportPath = useEditorStore((state) => state.chooseExportPath);
  const exportPhoto = useEditorStore((state) => state.exportPhoto);
  const exportJob = useEditorStore((state) => state.exportJob);
  const cancelExport = useEditorStore((state) => state.cancelExport);
  const watermarkEnabled = useSettingsStore((state) => state.watermarkEnabled);
  const watermarkKind = useSettingsStore((state) => state.watermarkKind);
  const watermarkText = useSettingsStore((state) => state.watermarkText);
  const watermarkImage = useSettingsStore((state) => state.watermarkImage);
  const watermarkOpacity = useSettingsStore((state) => state.watermarkOpacity);
  const watermarkScale = useSettingsStore((state) => state.watermarkScale);
  const watermarkPosition = useSettingsStore((state) => state.watermarkPosition);
  const updateSettings = useSettingsStore((state) => state.update);

  const [format, setFormat] = useState<ExportFormat>("jpeg");
  const [quality, setQuality] = useState(90);
  const [size, setSize] = useState<ExportSize>("original");
  const [colorSpace, setColorSpace] = useState<ExportColorSpace>("srgb");
  const [metadata, setMetadata] = useState<ExportMetadata>("keep");
  const [bitDepth, setBitDepth] = useState<ExportBitDepth>("eight");
  const [customWidth, setCustomWidth] = useState(2048);
  const [customHeight, setCustomHeight] = useState(2048);
  const [exporting, setExporting] = useState(false);
  const [logoBusy, setLogoBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const dialogRef = useRef<HTMLDivElement>(null);
  const previousFocus = useRef<HTMLElement | null>(null);
  const titleId = useId();

  useEffect(() => {
    if (!open) return;
    previousFocus.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const settings = useSettingsStore.getState();
    setFormat(settings.exportFormat === "tiff" && mode !== "tauri" ? "jpeg" : settings.exportFormat);
    setQuality(settings.exportQuality);
    setSize(settings.exportSize);
    setColorSpace(settings.exportColorSpace);
    setMetadata(settings.exportMetadata);
    setBitDepth(settings.exportBitDepth);
    setCustomWidth(settings.exportCustomWidth);
    setCustomHeight(settings.exportCustomHeight);
    setExporting(false);
    setError(null);
    const timer = window.setTimeout(() => dialogRef.current?.focus(), 30);
    return () => {
      window.clearTimeout(timer);
      previousFocus.current?.focus();
    };
  }, [open]);

  if (!open || !doc?.source) return null;

  const tiffAvailable = mode === "tauri";
  const baseWidth = doc.crop?.width || doc.source.width;
  const baseHeight = doc.crop?.height || doc.source.height;
  const out = computeOutputDimensions(baseWidth, baseHeight, size, customWidth, customHeight);
  // Width/height tokens describe the exported pixels, so the stem is rendered
  // after the dimensions are known.
  const stem = renderExportStem(useSettingsStore.getState().exportNameTemplate, {
    name: doc.name,
    collection: doc.collectionName,
    width: out.width,
    height: out.height,
  });
  const fileName = `${stem}.${extensionForFormat(format)}`;
  const customInvalid =
    size === "custom" &&
    (out.width < 16 || out.height < 16 || out.width * out.height > MAX_OUTPUT_PIXELS);
  const watermarkReady =
    watermarkKind === "text" ? watermarkText.trim().length > 0 : Boolean(watermarkImage);
  const watermarkInvalid = watermarkEnabled && !watermarkReady;

  const handleClose = (): void => {
    if (exporting) return;
    close();
  };

  const handlePickLogo = async (): Promise<void> => {
    if (logoBusy) return;
    setLogoBusy(true);
    try {
      const file = await pickFile(".png,.jpg,.jpeg,.webp,image/png,image/jpeg,image/webp");
      if (!file) return;
      if (file.size > 1_500_000) {
        setError("水印 Logo 请控制在 1.5MB 以内。");
        return;
      }
      const dataUrl = await readAsDataUrl(file);
      if (dataUrl.length > MAX_WATERMARK_DATA_URL) {
        setError("水印 Logo 过大，请换一张更小的图片。");
        return;
      }
      updateSettings({
        watermarkImage: dataUrl,
        watermarkKind: "image",
        watermarkEnabled: true,
      });
      setError(null);
    } catch {
      setError("无法读取水印 Logo，请重试。");
    } finally {
      setLogoBusy(false);
    }
  };

  const handleExport = async (): Promise<void> => {
    if (exporting || customInvalid) return;
    setExporting(true);
    setError(null);
    try {
      const path = await chooseExportPath(fileName, format);
      if (!path) {
        setExporting(false);
        return;
      }
      const watermark = await buildWatermarkOptions(useSettingsStore.getState());
      if (watermarkEnabled && !watermark) {
        setError("请先输入水印文字或选择 Logo。");
        setExporting(false);
        return;
      }
      const result = await exportPhoto({
        path,
        format,
        quality,
        maxDimension: null,
        width: size === "original" ? null : out.width,
        height: size === "original" ? null : out.height,
        colorSpace,
        metadata,
        bitDepth,
        watermark,
      });
      const requestedLabel =
        colorSpace === "srgb" ? "sRGB" : colorSpace === "displayP3" ? "Display P3" : "原图配置";
      const fallbackNote =
        result.colorSpace === requestedLabel
          ? ""
          : `，${requestedLabel} 不可用，已改按 ${result.colorSpace} 写出`;
      const metadataNote =
        metadata === "none"
          ? ""
          : result.metadataWritten
            ? result.keepsLocation
              ? "，已保留拍摄参数（含位置）"
              : "，已保留拍摄参数"
            : "，该格式无法写入拍摄参数";
      toastSuccess(
        mode === "demo"
          ? `照片已保存到下载文件夹：${fileName}。原图仍在，不会被覆盖。`
          : `已导出到 ${result.path}（${result.backend === "wgpu" ? "GPU 渲染" : "CPU 渲染"}，${result.colorSpace} · ${result.bitDepth} 位${fallbackNote}${metadataNote}）`,
        "照片已导出",
      );
      setExporting(false);
      close();
    } catch (exportError) {
      if (isApiError(exportError) && exportError.code === "EXPORT_CANCELLED") {
        setError(null);
        setExporting(false);
        return;
      }
      setError(userMessageOf(exportError, "导出没有完成，请换一个位置或格式再试。"));
      setExporting(false);
    }
  };

  const onDialogKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>): void => {
    if (event.key === "Escape") {
      event.stopPropagation();
      handleClose();
      return;
    }
    if (event.key !== "Tab") return;
    const dialog = dialogRef.current;
    if (!dialog) return;
    const focusables = Array.from(
      dialog.querySelectorAll<HTMLElement>(
        'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
      ),
    );
    if (focusables.length === 0) return;
    const first = focusables[0];
    const last = focusables[focusables.length - 1];
    if (event.shiftKey && (document.activeElement === first || document.activeElement === dialog)) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  };

  return createPortal(
    <div
      className="modal-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) handleClose();
      }}
    >
      <div
        ref={dialogRef}
        className="export-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        onKeyDown={onDialogKeyDown}
      >
        <header className="export-dialog__head">
          <div>
            <p className="export-dialog__eyebrow">EXPORT</p>
            <h2 id={titleId}>导出照片</h2>
            <p>选择格式和尺寸，生成一份新的照片文件。</p>
          </div>
          <button
            type="button"
            className="modal-close"
            aria-label="关闭导出窗口"
            title="关闭"
            disabled={exporting}
            onClick={handleClose}
          >
            <X size={15} />
          </button>
        </header>

        <div className="export-dialog__body">
          <section className="export-section">
            <div className="export-section__head">
              <span>格式</span>
              {!tiffAvailable ? <span className="section-hint">TIFF 仅在桌面版可用</span> : null}
            </div>
            <div className="format-grid format-grid--four" role="group" aria-label="导出格式">
              {FORMATS.map((item) => {
                const disabled = item.value === "tiff" && !tiffAvailable;
                return (
                  <button
                    key={item.value}
                    type="button"
                    className={cn("format-card", format === item.value && "is-active", disabled && "is-disabled")}
                    aria-pressed={format === item.value}
                    disabled={disabled}
                    title={disabled ? "TIFF 导出仅在桌面版可用" : item.description}
                    onClick={() => setFormat(item.value)}
                  >
                    <span className="format-card__name">{item.label}</span>
                    <span className="format-card__desc">{item.description}</span>
                    {disabled ? <span className="format-card__badge">桌面版支持</span> : null}
                  </button>
                );
              })}
            </div>
          </section>

          <section className="export-section">
            <div className="export-section__head">
              <span>色彩空间</span>
              <span className="section-hint">
                {doc.source.hasSourceProfile
                  ? `原图为 ${doc.source.colorSpace}，已在工作空间处理`
                  : `原图为 ${doc.source.colorSpace || "sRGB"}`}
              </span>
            </div>
            <div className="format-grid" role="group" aria-label="导出色彩空间">
              {COLOR_SPACES.map((item) => {
                const needsProfile = item.value === "source";
                const disabled =
                  (needsProfile && !doc.source?.hasSourceProfile) ||
                  (item.value !== "srgb" && (mode === "demo" || format === "webp"));
                const title = disabled
                  ? needsProfile
                    ? "这张照片没有可保留的源配置"
                    : format === "webp"
                      ? "WebP 导出仅支持 sRGB"
                      : "浏览器演示模式只导出 sRGB"
                  : item.description;
                return (
                  <button
                    key={item.value}
                    type="button"
                    className={cn(
                      "format-card",
                      colorSpace === item.value && "is-active",
                      disabled && "is-disabled",
                    )}
                    aria-pressed={colorSpace === item.value}
                    disabled={disabled}
                    title={title}
                    onClick={() => setColorSpace(item.value)}
                  >
                    <span className="format-card__name">{item.label}</span>
                    <span className="format-card__desc">{item.description}</span>
                  </button>
                );
              })}
            </div>
          </section>

          <section className="export-section">
            <div className="export-section__head">
              <span>拍摄信息</span>
              <span className="section-hint">方向与旧缩略图始终会移除</span>
            </div>
            <div className="format-grid" role="group" aria-label="导出元数据">
              {METADATA_MODES.map((item) => (
                <button
                  key={item.value}
                  type="button"
                  className={cn(
                    "format-card",
                    metadata === item.value && "is-active",
                    mode === "demo" && "is-disabled",
                  )}
                  aria-pressed={metadata === item.value}
                  disabled={mode === "demo" || format === "webp"}
                  title={
                    format === "webp"
                      ? "WebP 导出不写入拍摄信息"
                      : mode === "demo"
                        ? "浏览器演示模式导出的文件不含 EXIF"
                        : item.description
                  }
                  onClick={() => setMetadata(item.value)}
                >
                  <span className="format-card__name">{item.label}</span>
                  <span className="format-card__desc">{item.description}</span>
                </button>
              ))}
            </div>
          </section>

          <section className="export-section">
            <div className="export-section__head">
              <span>位深</span>
              <span className="section-hint">
                {format === "jpeg" || format === "webp"
                  ? `${format === "jpeg" ? "JPEG" : "WebP"} 仅支持 8 位`
                  : "16 位保留更多暗部层次"}
              </span>
            </div>
            <div className="format-grid" role="group" aria-label="导出位深">
              {(["eight", "sixteen"] as const).map((value) => {
                const disabled =
                  format === "jpeg" || format === "webp" || mode === "demo";
                return (
                  <button
                    key={value}
                    type="button"
                    className={cn(
                      "format-card",
                      bitDepth === value && "is-active",
                      disabled && "is-disabled",
                    )}
                    aria-pressed={bitDepth === value}
                    disabled={disabled}
                    title={
                      disabled
                        ? format === "jpeg"
                          ? "JPEG 不支持 16 位导出"
                          : format === "webp"
                            ? "WebP 不支持 16 位导出"
                            : "浏览器演示模式只导出 8 位"
                        : value === "sixteen"
                          ? "走 CPU 线性管线，文件更大"
                          : "通用 8 位输出"
                    }
                    onClick={() => setBitDepth(value)}
                  >
                    <span className="format-card__name">
                      {value === "eight" ? "8 位" : "16 位"}
                    </span>
                    <span className="format-card__desc">
                      {value === "eight" ? "体积更小，兼容最广" : "暗部过渡更平滑"}
                    </span>
                  </button>
                );
              })}
            </div>
          </section>

          <section className="export-section">
            <div className="export-section__head">
              <span>水印</span>
              <span className="section-hint">只写入导出文件，原图与工程不受影响</span>
            </div>
            <Toggle
              label="添加水印"
              checked={watermarkEnabled}
              description="文字会按当前照片的短边比例缩放，适合交付预览或社交分享"
              onChange={(checked) => updateSettings({ watermarkEnabled: checked })}
            />
            {watermarkEnabled ? (
              <div className="watermark-editor">
                <div className="watermark-row">
                  <Segmented<WatermarkKind>
                    value={watermarkKind}
                    options={WATERMARK_KIND_OPTIONS}
                    onChange={(kind) => updateSettings({ watermarkKind: kind })}
                    ariaLabel="水印类型"
                    size="sm"
                  />
                  {watermarkKind === "image" ? (
                    <div className="watermark-logo">
                      {watermarkImage ? (
                        <img
                          src={watermarkImage}
                          alt="水印 Logo 预览"
                          className="watermark-logo__preview"
                        />
                      ) : (
                        <span className="section-hint">推荐透明背景 PNG</span>
                      )}
                      <Button
                        variant="ghost"
                        size="sm"
                        icon={<ImagePlus size={13} />}
                        loading={logoBusy}
                        onClick={() => void handlePickLogo()}
                      >
                        {watermarkImage ? "更换 Logo" : "选择 Logo"}
                      </Button>
                      {watermarkImage ? (
                        <Button
                          variant="ghost"
                          size="sm"
                          icon={<Trash2 size={13} />}
                          onClick={() => updateSettings({ watermarkImage: null })}
                        >
                          移除
                        </Button>
                      ) : null}
                    </div>
                  ) : null}
                </div>
                {watermarkKind === "text" ? (
                  <input
                    className="input watermark-text"
                    value={watermarkText}
                    maxLength={120}
                    placeholder="例如：Luma Studio · 微信 luma-photo"
                    aria-label="水印文字"
                    onChange={(event) => updateSettings({ watermarkText: event.target.value })}
                  />
                ) : null}
                <div className="watermark-sliders">
                  <Slider
                    label="不透明度"
                    value={watermarkOpacity}
                    min={5}
                    max={100}
                    step={1}
                    defaultValue={60}
                    unit="%"
                    compact
                    onChange={(value) => updateSettings({ watermarkOpacity: value })}
                  />
                  <Slider
                    label="大小"
                    value={watermarkScale}
                    min={5}
                    max={60}
                    step={1}
                    defaultValue={18}
                    unit="%"
                    compact
                    hint="占导出照片短边的比例"
                    onChange={(value) => updateSettings({ watermarkScale: value })}
                  />
                </div>
                <div className="watermark-position">
                  <span className="section-hint">位置</span>
                  <Segmented<WatermarkPositionId>
                    value={watermarkPosition}
                    options={WATERMARK_POSITION_OPTIONS}
                    onChange={(position) => updateSettings({ watermarkPosition: position })}
                    ariaLabel="水印位置"
                    size="sm"
                  />
                </div>
                {watermarkInvalid ? (
                  <p className="field-hint watermark-warning">
                    水印已开启，请先输入文字或选择 Logo。
                  </p>
                ) : null}
              </div>
            ) : null}
          </section>

          {format === "jpeg" ? (
            <section className="export-section">
              <Slider
                label="画质"
                value={quality}
                min={1}
                max={100}
                step={1}
                defaultValue={90}
                unit="%"
                hint="JPEG 画质越高，文件越大"
                onChange={setQuality}
              />
            </section>
          ) : null}

          <section className="export-section">
            <div className="export-section__head">
              <span>输出尺寸</span>
              <span className="section-hint">
                {formatDimensions(out.width, out.height)} ·{" "}
                {size === "original"
                  ? "当前裁剪尺寸"
                  : size === "custom"
                    ? "按宽高居中裁切"
                    : "按比例居中裁切"}
              </span>
            </div>
            <div className="size-grid size-grid--export" role="group" aria-label="输出尺寸">
              {EXPORT_SIZE_OPTIONS.map((item) => (
                <button
                  key={item.value}
                  type="button"
                  className={cn("size-card", size === item.value && "is-active")}
                  aria-pressed={size === item.value}
                  title={item.hint}
                  onClick={() => setSize(item.value)}
                >
                  <span>{item.label}</span>
                  <small>{item.hint}</small>
                </button>
              ))}
            </div>
            {size === "custom" ? (
              <div className="custom-size-row custom-size-row--export">
                <label>
                  <span>宽</span>
                  <DimensionInput
                    value={customWidth}
                    ariaLabel="导出宽度"
                    onChange={setCustomWidth}
                  />
                </label>
                <span className="custom-size-row__times" aria-hidden="true">
                  ×
                </span>
                <label>
                  <span>高</span>
                  <DimensionInput
                    value={customHeight}
                    ariaLabel="导出高度"
                    onChange={setCustomHeight}
                  />
                </label>
                <span className="custom-size-row__unit">px</span>
              </div>
            ) : null}
            {size !== "original" ? (
              <p className="field-hint">
                比例不一致时会居中裁切而不是拉伸；自定义尺寸超过当前画面时会放大，可能变软。
              </p>
            ) : null}
          </section>

          <div className="export-file">
            <span className="export-file__icon">
              <FileImage size={15} />
            </span>
            <div>
              <span>保存为</span>
              <p>{fileName}</p>
            </div>
          </div>

          {exporting ? (
            <div className="export-progress" role="status" aria-live="polite">
              <div className="export-progress__head">
                <span>{exportJob?.message ?? "正在导出…"}</span>
                <span className="mono">
                  {exportJob?.percent !== null && exportJob?.percent !== undefined
                    ? `${Math.round(exportJob.percent)}%`
                    : "…"}
                </span>
              </div>
              <div className="export-progress__track">
                <div
                  className="export-progress__fill"
                  style={{ width: `${exportJob?.percent ?? 8}%` }}
                />
              </div>
              <p>可随时取消；取消不会覆盖原图或留下未完成的文件。</p>
            </div>
          ) : null}

          {error ? (
            <p className="export-error" role="alert">
              <AlertTriangle size={13} aria-hidden="true" />
              {error}
            </p>
          ) : null}

          <div className="export-reassure">
            <LockKeyhole size={13} aria-hidden="true" />
            <span>原图始终保留，不会被覆盖。导出只会生成一份新文件。</span>
          </div>
        </div>

        <footer className="export-dialog__foot">
          <Button
            variant="ghost"
            size="md"
            onClick={() => {
              if (exporting) void cancelExport();
              else handleClose();
            }}
          >
            {exporting ? "取消导出" : "取消"}
          </Button>
          <Button
            variant="primary"
            size="md"
            icon={<Download size={14} />}
            loading={exporting}
            disabled={customInvalid || watermarkInvalid}
            onClick={() => void handleExport()}
          >
            {exporting ? "正在导出…" : "导出照片"}
          </Button>
        </footer>
      </div>
    </div>,
    document.body,
  );
}
