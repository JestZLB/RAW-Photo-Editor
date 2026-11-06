import {
  useEffect,
  useId,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";
import { createPortal } from "react-dom";
import {
  Copy,
  Cpu,
  Gauge,
  HardDrive,
  MonitorCog,
  Palette,
  Pencil,
  RotateCcw,
  ScanFace,
  ScanLine,
  Sparkles,
  Trash2,
  Wand2,
  X,
} from "lucide-react";
import { userMessageOf } from "../api/errors";
import type { ExportFormat } from "../api/types";
import { cn } from "../lib/cn";
import { EXPORT_SIZE_OPTIONS } from "../lib/export";
import { formatBytes, formatDimensions, formatRelativeTime } from "../lib/format";
import {
  EXPORT_COLOR_SPACES,
  EXPORT_BIT_DEPTHS,
  EXPORT_METADATA_MODES,
  useSettingsStore,
  type PortraitIntensity,
  type PreviewMaxDimension,
  type ThemeId,
} from "../stores/settingsStore";
import { useEditorStore } from "../stores/editorStore";
import { toastError, toastSuccess } from "../stores/toastStore";
import { Button } from "./controls/Button";
import { DimensionInput } from "./controls/DimensionInput";
import { Segmented } from "./controls/Segmented";
import { Slider } from "./controls/Slider";
import { Toggle } from "./controls/Toggle";

type PreviewValue = `${PreviewMaxDimension}`;
type IntensityValue = `${PortraitIntensity}`;

const THEME_OPTIONS: Array<{
  id: ThemeId;
  label: string;
  description: string;
  bg: string;
  surface: string;
  accent: string;
}> = [
  {
    id: "ink",
    label: "墨蓝",
    description: "冷蓝主色，默认",
    bg: "#0b0e13",
    surface: "#121821",
    accent: "#83b5ff",
  },
  {
    id: "sand",
    label: "暖砂",
    description: "焦糖主色，偏胶片",
    bg: "#14100d",
    surface: "#1c1713",
    accent: "#e9b384",
  },
  {
    id: "graphite",
    label: "石墨",
    description: "中性灰 + 青绿",
    bg: "#0e1011",
    surface: "#15181a",
    accent: "#7fd0c4",
  },
  {
    id: "pine",
    label: "松柏",
    description: "墨绿 + 薄荷",
    bg: "#0a1210",
    surface: "#101a17",
    accent: "#8fd9b6",
  },
  {
    id: "indigo",
    label: "夜紫",
    description: "紫灰 + 薰衣草",
    bg: "#0e0c15",
    surface: "#16131f",
    accent: "#b0a2ff",
  },
  {
    id: "light",
    label: "浅色",
    description: "明亮工作台",
    bg: "#f3f4f6",
    surface: "#fbfbfc",
    accent: "#2f65c8",
  },
];

const PREVIEW_OPTIONS: Array<{ value: PreviewValue; label: string; title: string }> = [
  { value: "1200", label: "流畅", title: "最长边 1200 像素，CPU 负担最低" },
  { value: "1600", label: "标准", title: "最长边 1600 像素，清晰度与速度平衡" },
  { value: "2048", label: "精细", title: "最长边 2048 像素，更清晰但渲染更慢" },
];

const INTENSITY_OPTIONS: Array<{ value: IntensityValue; label: string; title: string }> = [
  { value: "55", label: "轻一点", title: "更克制，几乎看不出调整" },
  { value: "78", label: "适中", title: "日常照片的稳妥选择" },
  { value: "100", label: "明显", title: "效果更突出" },
];

const FORMATS: Array<{ value: ExportFormat; label: string; description: string }> = [
  { value: "jpeg", label: "JPEG", description: "体积小，适合分享" },
  { value: "png", label: "PNG", description: "无损，适合存档" },
  { value: "tiff", label: "TIFF", description: "印刷级，桌面版支持" },
];


export function SettingsDialog() {
  const open = useEditorStore((state) => state.settingsOpen);
  const close = useEditorStore((state) => state.closeSettings);
  const doc = useEditorStore((state) => state.doc);
  const mode = useEditorStore((state) => state.mode);
  const appInfo = useEditorStore((state) => state.appInfo);
  const providers = useEditorStore((state) => state.providers);
  const recent = useEditorStore((state) => state.recent);
  const setTool = useEditorStore((state) => state.setTool);
  const refreshPreview = useEditorStore((state) => state.refreshPreview);
  const renameProject = useEditorStore((state) => state.renameProject);
  const deleteProject = useEditorStore((state) => state.deleteProject);
  const clearRenderCache = useEditorStore((state) => state.clearRenderCache);
  const api = useEditorStore((state) => state.api);
  const settings = useSettingsStore();

  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [renameText, setRenameText] = useState("");
  const [deletingId, setDeletingId] = useState<string | null>(null);
  const [cacheBusy, setCacheBusy] = useState(false);
  const renameCommittedRef = useRef<string | null>(null);
  const dialogRef = useRef<HTMLDivElement>(null);
  const previousFocus = useRef<HTMLElement | null>(null);
  const titleId = useId();

  useEffect(() => {
    if (!open) return;
    previousFocus.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    setRenamingId(null);
    setDeletingId(null);
    setCacheBusy(false);
    const timer = window.setTimeout(() => dialogRef.current?.focus(), 30);
    return () => {
      window.clearTimeout(timer);
      previousFocus.current?.focus();
    };
  }, [open]);

  if (!open) return null;

  const tiffAvailable = mode === "tauri";
  const previewValue = `${settings.previewMaxDimension}` as PreviewValue;
  const intensityValue = `${settings.portraitIntensity}` as IntensityValue;

  const handlePreviewChange = (value: PreviewValue): void => {
    settings.update({ previewMaxDimension: Number(value) as PreviewMaxDimension });
    if (doc) refreshPreview();
  };

  const handleFormat = (format: ExportFormat): void => {
    if (format === "tiff" && !tiffAvailable) return;
    settings.update({ exportFormat: format });
  };

  const handleReset = (): void => {
    settings.reset();
    if (doc) refreshPreview();
    toastSuccess("预览、导出与人像偏好已回到默认值。", "设置已恢复默认");
  };

  const handleCopyPath = async (): Promise<void> => {
    const path = appInfo?.workspaceDir;
    if (!path) return;
    try {
      await navigator.clipboard.writeText(path);
      toastSuccess("工作区路径已复制到剪贴板。", "已复制");
    } catch {
      toastError("无法访问剪贴板，请手动选择路径复制。");
    }
  };

  const openAiPanel = (): void => {
    if (!doc) return;
    close();
    setTool("ai");
  };

  const denoiseModelName = settings.aiDenoiseModelPath
    ? (settings.aiDenoiseModelPath.split(/[\\/]/).pop() ?? settings.aiDenoiseModelPath)
    : "未选择模型";

  const handlePickDenoiseModel = async (): Promise<void> => {
    if (!api) return;
    try {
      const path = await api.pickDenoiseModel();
      if (path) settings.update({ aiDenoiseModelPath: path });
    } catch (error) {
      toastError(userMessageOf(error, "无法打开文件选择窗口，请重试。"));
    }
  };

  const segmentationModelName = settings.segmentationModelPath
    ? (settings.segmentationModelPath.split(/[\\/]/).pop() ??
      settings.segmentationModelPath)
    : "未选择模型";

  const handlePickSegmentationModel = async (): Promise<void> => {
    if (!api) return;
    try {
      const path = await api.pickDenoiseModel();
      if (path) settings.update({ segmentationModelPath: path });
    } catch (error) {
      toastError(userMessageOf(error, "无法打开文件选择窗口，请重试。"));
    }
  };

  const startRename = (projectId: string, name: string): void => {
    setDeletingId(null);
    setRenamingId(projectId);
    setRenameText(name);
    renameCommittedRef.current = null;
  };

  const commitRename = async (projectId: string, originalName: string): Promise<void> => {
    if (renameCommittedRef.current === projectId) return;
    const name = renameText.trim();
    if (!name || name === originalName) {
      setRenamingId(null);
      return;
    }
    renameCommittedRef.current = projectId;
    const ok = await renameProject(projectId, name);
    if (ok) setRenamingId(null);
    else renameCommittedRef.current = null;
  };

  const confirmDelete = async (projectId: string): Promise<void> => {
    const ok = await deleteProject(projectId);
    if (ok) setDeletingId(null);
  };

  const handleClearCache = async (): Promise<void> => {
    setCacheBusy(true);
    try {
      const result = await clearRenderCache();
      if (result) {
        toastSuccess(
          result.deletedFiles === 0
            ? "没有旧预览需要清理。"
            : `已清理 ${result.deletedFiles} 个预览文件，释放 ${formatBytes(result.freedBytes)}。`,
          "缓存已清理",
        );
      }
    } finally {
      setCacheBusy(false);
    }
  };

  const onDialogKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>): void => {
    if (event.key === "Escape") {
      event.stopPropagation();
      close();
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
        if (event.target === event.currentTarget) close();
      }}
    >
      <div
        ref={dialogRef}
        className="export-dialog settings-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        onKeyDown={onDialogKeyDown}
      >
        <header className="export-dialog__head">
          <div>
            <p className="export-dialog__eyebrow">PREFERENCES</p>
            <h2 id={titleId}>设置</h2>
            <p>偏好保存在这台电脑上，只影响之后的操作。</p>
          </div>
          <button
            type="button"
            className="modal-close"
            aria-label="关闭设置"
            title="关闭"
            onClick={close}
          >
            <X size={15} />
          </button>
        </header>

        <div className="export-dialog__body settings-body">
          <section className="settings-section">
            <div className="settings-section__head">
              <div>
                <h3>
                  <Gauge size={14} aria-hidden="true" />
                  预览画质
                </h3>
                <p>窗口预览的最长边，越大越清晰，CPU 渲染也越慢。</p>
              </div>
              <Segmented
                value={previewValue}
                options={PREVIEW_OPTIONS}
                onChange={handlePreviewChange}
                ariaLabel="预览画质"
                size="sm"
              />
            </div>
            <p className="field-hint">
              当前预览 {settings.previewMaxDimension} px；修改后会立即重新生成预览，导出不受影响。
            </p>
          </section>

          <section className="settings-section">
            <div className="settings-section__head settings-section__head--stacked">
              <div>
                <h3>
                  <Palette size={14} aria-hidden="true" />
                  主题外观
                </h3>
                <p>只影响界面配色，照片的渲染、预览与导出不受影响。</p>
              </div>
            </div>
            <div className="theme-picker" role="group" aria-label="主题">
              {THEME_OPTIONS.map((theme) => {
                const active = settings.theme === theme.id;
                return (
                  <button
                    key={theme.id}
                    type="button"
                    className={cn("theme-swatch", active && "is-active")}
                    aria-pressed={active}
                    title={theme.description}
                    onClick={() => settings.update({ theme: theme.id })}
                  >
                    <span className="theme-swatch__dots" aria-hidden="true">
                      <span style={{ background: theme.bg }} />
                      <span style={{ background: theme.surface }} />
                      <span style={{ background: theme.accent }} />
                    </span>
                    <span className="theme-swatch__name">{theme.label}</span>
                  </button>
                );
              })}
            </div>
          </section>

          <section className="settings-section">
            <div className="settings-section__head settings-section__head--stacked">
              <div>
                <h3>
                  <Sparkles size={14} aria-hidden="true" />
                  人像预设程度
                </h3>
                <p>应用快速风格或恢复默认时使用此程度，不修改已有图层。</p>
              </div>
              <Segmented
                value={intensityValue}
                options={INTENSITY_OPTIONS}
                onChange={(value) =>
                  settings.update({ portraitIntensity: Number(value) as PortraitIntensity })
                }
                ariaLabel="人像预设程度"
                size="sm"
              />
            </div>
          </section>

          <section className="settings-section">
            <div className="settings-section__head settings-section__head--stacked">
              <div>
                <h3>
                  <Cpu size={14} aria-hidden="true" />
                  导出默认值
                </h3>
                <p>打开导出窗口时使用这些默认值，导出时仍可临时修改。</p>
              </div>
            </div>

            <div className="format-grid" role="group" aria-label="默认导出格式">
              {FORMATS.map((item) => {
                const disabled = item.value === "tiff" && !tiffAvailable;
                return (
                  <button
                    key={item.value}
                    type="button"
                    className={cn(
                      "format-card",
                      settings.exportFormat === item.value && "is-active",
                      disabled && "is-disabled",
                    )}
                    aria-pressed={settings.exportFormat === item.value}
                    disabled={disabled}
                    title={disabled ? "TIFF 导出仅在桌面版可用" : item.description}
                    onClick={() => handleFormat(item.value)}
                  >
                    <span className="format-card__name">{item.label}</span>
                    <span className="format-card__desc">{item.description}</span>
                    {disabled ? <span className="format-card__badge">桌面版支持</span> : null}
                  </button>
                );
              })}
            </div>

            <div className="settings-subsection">
              <span className="settings-subsection__label">默认导出色彩空间</span>
              <div className="format-grid" role="group" aria-label="默认导出色彩空间">
                {EXPORT_COLOR_SPACES.map((value) => {
                  const disabled = value !== "srgb" && !tiffAvailable;
                  return (
                    <button
                      key={value}
                      type="button"
                      className={cn(
                        "format-card",
                        settings.exportColorSpace === value && "is-active",
                        disabled && "is-disabled",
                      )}
                      aria-pressed={settings.exportColorSpace === value}
                      disabled={disabled}
                      title={
                        disabled
                          ? "浏览器演示模式只导出 sRGB"
                          : value === "source"
                            ? "按照片自带的 ICC 配置导出，没有配置时回退 sRGB"
                            : value === "displayP3"
                              ? "写入 Display P3 配置，广色域屏幕更鲜艳"
                              : "通用兼容，未标注配置"
                      }
                      onClick={() => settings.update({ exportColorSpace: value })}
                    >
                      <span className="format-card__name">
                        {value === "srgb" ? "sRGB" : value === "displayP3" ? "Display P3" : "原图配置"}
                      </span>
                    </button>
                  );
                })}
              </div>
            </div>

            <div className="settings-subsection">
              <span className="settings-subsection__label">默认元数据与位深</span>
              <div className="format-grid" role="group" aria-label="默认导出元数据">
                {EXPORT_METADATA_MODES.map((value) => (
                  <button
                    key={value}
                    type="button"
                    className={cn(
                      "format-card",
                      settings.exportMetadata === value && "is-active",
                    )}
                    aria-pressed={settings.exportMetadata === value}
                    onClick={() => settings.update({ exportMetadata: value })}
                  >
                    <span className="format-card__name">
                      {value === "keep" ? "保留拍摄信息" : value === "privacy" ? "去除位置" : "不写入"}
                    </span>
                    <span className="format-card__desc">
                      {value === "keep"
                        ? "含 GPS 等位置信息"
                        : value === "privacy"
                          ? "分享前更稳妥"
                          : "只留下画面本身"}
                    </span>
                  </button>
                ))}
              </div>
              <div className="format-grid" role="group" aria-label="默认导出位深">
                {EXPORT_BIT_DEPTHS.map((value) => (
                  <button
                    key={value}
                    type="button"
                    className={cn(
                      "format-card",
                      settings.exportBitDepth === value && "is-active",
                    )}
                    aria-pressed={settings.exportBitDepth === value}
                    onClick={() => settings.update({ exportBitDepth: value })}
                  >
                    <span className="format-card__name">
                      {value === "eight" ? "8 位" : "16 位"}
                    </span>
                    <span className="format-card__desc">
                      {value === "eight" ? "JPEG/PNG/TIFF 通用" : "仅 PNG 与 TIFF"}
                    </span>
                  </button>
                ))}
              </div>
            </div>

            {settings.exportFormat === "jpeg" ? (
              <div className="settings-quality">
                <Slider
                  label="默认画质"
                  value={settings.exportQuality}
                  min={1}
                  max={100}
                  step={1}
                  defaultValue={90}
                  unit="%"
                  hint="JPEG 画质越高，文件越大"
                  onChange={(value) => settings.update({ exportQuality: value })}
                />
              </div>
            ) : null}

            <div className="settings-section__label">默认尺寸</div>
            <div className="size-grid size-grid--export" role="group" aria-label="默认导出尺寸">
              {EXPORT_SIZE_OPTIONS.map((item) => (
                <button
                  key={item.value}
                  type="button"
                  className={cn("size-card", settings.exportSize === item.value && "is-active")}
                  aria-pressed={settings.exportSize === item.value}
                  title={item.hint}
                  onClick={() => settings.update({ exportSize: item.value })}
                >
                  <span>{item.label}</span>
                  <small>{item.hint}</small>
                </button>
              ))}
            </div>
            {settings.exportSize === "custom" ? (
              <div className="custom-size-row">
                <label>
                  <span>宽</span>
                  <DimensionInput
                    value={settings.exportCustomWidth}
                    ariaLabel="默认导出宽度"
                    onChange={(value) => settings.update({ exportCustomWidth: value })}
                  />
                </label>
                <span className="custom-size-row__times" aria-hidden="true">
                  ×
                </span>
                <label>
                  <span>高</span>
                  <DimensionInput
                    value={settings.exportCustomHeight}
                    ariaLabel="默认导出高度"
                    onChange={(value) => settings.update({ exportCustomHeight: value })}
                  />
                </label>
                <span className="custom-size-row__unit">px</span>
              </div>
            ) : null}
            <p className="field-hint">
              比例预设会在当前裁剪范围内居中裁切，不拉伸；自定义尺寸比例不一致时同样居中裁切。
            </p>

            <div className="settings-section__label">导出文件名</div>
            <input
              className="input"
              value={settings.exportNameTemplate}
              maxLength={120}
              spellCheck={false}
              aria-label="导出文件名模板"
              onChange={(event) =>
                settings.update({ exportNameTemplate: event.currentTarget.value })
              }
            />
            <p className="field-hint">
              可用占位符：{"{name} {collection} {date} {time} {index} {width} {height}"}
              ；扩展名按导出格式自动追加，批量导出会自动去重。
            </p>
          </section>

          <section className="settings-section">
            <div className="settings-section__head settings-section__head--stacked">
              <div>
                <h3>
                  <MonitorCog size={14} aria-hidden="true" />
                  界面
                </h3>
                <p>减少等待时的视觉干扰。</p>
              </div>
            </div>
            <Toggle
              label="减少动画"
              description="关闭淡入、弹窗缩放和提示动画；不影响编辑结果。"
              checked={settings.reduceMotion}
              onChange={(checked) => settings.update({ reduceMotion: checked })}
            />
          </section>

          <section className="settings-section">
            <div className="settings-section__head settings-section__head--stacked">
              <div>
                <h3>
                  <HardDrive size={14} aria-hidden="true" />
                  照片与缓存
                </h3>
                <p>照片可以重命名或删除；删除会同时清理这张照片的本地资产。项目集在菜单栏中切换。</p>
              </div>
            </div>
            {recent.length === 0 ? (
              <p className="settings-model-list__empty">还没有最近项目。</p>
            ) : (
              <div className="settings-projects">
                {recent.slice(0, 10).map((project) => {
                  const isCurrent = doc?.id === project.id;
                  if (renamingId === project.id) {
                    return (
                      <div key={project.id} className="settings-project-row">
                        <div className="settings-project-main">
                          <input
                            className="input settings-project-input"
                            value={renameText}
                            maxLength={120}
                            autoFocus
                            aria-label={`重命名${project.name}`}
                            onChange={(event) => setRenameText(event.currentTarget.value)}
                            onKeyDown={(event) => {
                              if (event.key === "Enter") {
                                event.preventDefault();
                                void commitRename(project.id, project.name);
                              } else if (event.key === "Escape") {
                                event.preventDefault();
                                renameCommittedRef.current = project.id;
                                setRenamingId(null);
                              }
                            }}
                            onBlur={() => void commitRename(project.id, project.name)}
                          />
                          <span className="settings-project-meta">按 Enter 保存，Esc 取消</span>
                        </div>
                      </div>
                    );
                  }
                  return (
                    <div key={project.id} className="settings-project-row">
                      <div className="settings-project-main">
                        <span className="settings-project-name" title={project.name}>
                          {project.name}
                          {isCurrent ? <span className="settings-project-current">当前</span> : null}
                        </span>
                        <span className="settings-project-meta">
                          {project.collectionName} ·{" "}
                          {formatDimensions(project.width, project.height)} ·{" "}
                          {formatRelativeTime(project.updatedAt)}
                        </span>
                      </div>
                      {deletingId === project.id ? (
                        <div className="settings-project-confirm">
                          <span>{isCurrent ? "删除当前项目？" : "确认删除？"}</span>
                          <button
                            type="button"
                            className="settings-confirm-btn settings-confirm-btn--danger"
                            onClick={() => void confirmDelete(project.id)}
                          >
                            删除
                          </button>
                          <button
                            type="button"
                            className="settings-confirm-btn"
                            onClick={() => setDeletingId(null)}
                          >
                            取消
                          </button>
                        </div>
                      ) : (
                        <div className="settings-project-actions">
                          <button
                            type="button"
                            className="settings-icon-action"
                            aria-label={`重命名${project.name}`}
                            title="重命名"
                            onClick={() => startRename(project.id, project.name)}
                          >
                            <Pencil size={11} />
                          </button>
                          <button
                            type="button"
                            className="settings-icon-action settings-icon-action--danger"
                            aria-label={`删除${project.name}`}
                            title="删除项目"
                            onClick={() => setDeletingId(project.id)}
                          >
                            <Trash2 size={11} />
                          </button>
                        </div>
                      )}
                    </div>
                  );
                })}
              </div>
            )}
            {deletingId && recent.some((project) => project.id === deletingId) ? (
              <p className="field-hint">删除不可撤销；正在进行的 AI 任务需要先取消。</p>
            ) : null}
            <div className="settings-cache-row">
              <div>
                <strong>预览缓存</strong>
                <span>清理旧 revision 的渲染预览；原图、缩略图与 AI 结果不受影响。</span>
              </div>
              <Button
                variant="secondary"
                size="sm"
                loading={cacheBusy}
                onClick={() => void handleClearCache()}
              >
                清理预览缓存
              </Button>
            </div>
          </section>

          <section className="settings-section">
            <div className="settings-section__head settings-section__head--stacked">
              <div>
                <h3>
                  <Sparkles size={14} aria-hidden="true" />
                  AI 模型与算力
                </h3>
                <p>模型列表在 AI 面板中配置；可拉取服务商提供的可用模型。</p>
              </div>
            </div>
            <div className="settings-model-list">
              {providers.length === 0 ? (
                <p className="settings-model-list__empty">还没有可用的 AI 服务。</p>
              ) : (
                providers.map((provider) => (
                  <div key={provider.id} className="settings-model-row">
                    <span className="settings-model-row__name">{provider.name}</span>
                    <code>
                      {provider.kind === "mock" ? "luma-demo-v1" : provider.model ?? "未设置模型"}
                    </code>
                    <span className={cn("badge", provider.kind === "mock" ? "badge--green" : "badge--amber")}>
                      {provider.kind === "mock" ? "本地演示" : "云端 GPU"}
                    </span>
                  </div>
                ))
              )}
            </div>
            <Button
              variant="secondary"
              size="sm"
              disabled={!doc}
              onClick={openAiPanel}
            >
              在 AI 面板中配置模型
            </Button>
            <p className="field-hint">
              云端生成由服务商 GPU 执行。本地已接入 WGPU 渲染：基础照片、调色（不含清晰度/锐度）和
              AI 结果图层会在兼容文档上自动使用 GPU；含人像图层或清晰度/锐度调整的文档自动回退 CPU。
            </p>
          </section>

          <section className="settings-section">
            <div className="settings-section__head settings-section__head--stacked">
              <div>
                <h3>
                  <Wand2 size={14} aria-hidden="true" />
                  本地 AI 降噪
                </h3>
                <p>内置 Photo-HQ-W4xEX，或自选 ONNX 模型；推理全部在本机完成。</p>
              </div>
            </div>
            <p className="settings-section__label">默认引擎</p>
            <Segmented
              ariaLabel="默认 AI 降噪引擎"
              value={settings.aiDenoiseEngine}
              options={[
                {
                  value: "photoHq",
                  label: "Photo-HQ",
                  title: "内置 Real-ESRGAN 4x 人像降噪",
                },
                {
                  value: "onnx",
                  label: "自选 ONNX",
                  title: "使用你提供的 ONNX 降噪模型",
                },
              ]}
              onChange={(value) => settings.update({ aiDenoiseEngine: value })}
              disabled={mode !== "tauri"}
              size="sm"
            />
            <div className="settings-cache-row">
              <div>
                <strong>Photo-HQ-W4xEX</strong>
                <span>Real-ESRGAN ncnn · 4x · 本机 GPU</span>
              </div>
              <span
                className={cn("badge", mode === "tauri" ? "badge--green" : "badge--amber")}
              >
                {mode === "tauri" ? "已内置" : "仅桌面版"}
              </span>
            </div>
            <div className="settings-cache-row">
              <div>
                <strong title={settings.aiDenoiseModelPath ?? undefined}>{denoiseModelName}</strong>
                <span>
                  {settings.aiDenoiseModelPath
                    ? "ONNX 模型 · 已配置"
                    : "支持 waifu2x / RealCUGAN / SCUNet 风格的降噪模型"}
                </span>
              </div>
              <Button
                variant="secondary"
                size="sm"
                disabled={mode !== "tauri"}
                onClick={() => void handlePickDenoiseModel()}
              >
                {settings.aiDenoiseModelPath ? "更换模型" : "选择 .onnx 模型"}
              </Button>
              {settings.aiDenoiseModelPath ? (
                <button
                  type="button"
                  className="panel-link panel-link--inline"
                  onClick={() => settings.update({ aiDenoiseModelPath: null })}
                >
                  清除
                </button>
              ) : null}
            </div>
            <p className="field-hint">
              {mode === "tauri"
                ? "Photo-HQ 先按最长边 1600px 处理，再放大 4 倍并缩回原尺寸；ONNX 模型需输入 [1,3,H,W]、输出保持原尺寸。首次启用会自动检测本机能力，不可用时回退普通降噪。权重授权与适用场景由模型来源决定，应用只在本机读取文件。"
                : "浏览器演示模式没有本地推理能力，AI 降噪仅在桌面版可用。"}
            </p>
          </section>

          <section className="settings-section">
            <div className="settings-section__head settings-section__head--stacked">
              <div>
                <h3>
                  <ScanLine size={14} aria-hidden="true" />
                  智能蒙版（分割模型）
                </h3>
                <p>
                  应用不内置分割权重。选择一个本地 .onnx 后，调色/人像图层的蒙版区会出现「智能蒙版」。
                </p>
              </div>
              <span
                className={cn(
                  "badge",
                  settings.segmentationModelPath ? "badge--green" : "badge--amber",
                )}
              >
                {settings.segmentationModelPath ? "已配置" : "未配置"}
              </span>
            </div>
            <div className="settings-cache-row">
              <div>
                <strong title={settings.segmentationModelPath ?? undefined}>
                  {segmentationModelName}
                </strong>
                <span>
                  {settings.segmentationModelPath
                    ? "ONNX 模型 · 已配置"
                    : "输入 [1,3,512,512] RGB，输出 [1,1,H,W] 或 [1,2,H,W]（第二通道为前景）"}
                </span>
              </div>
              <Button
                variant="secondary"
                size="sm"
                disabled={mode !== "tauri"}
                onClick={() => void handlePickSegmentationModel()}
              >
                {settings.segmentationModelPath ? "更换模型" : "选择 .onnx 模型"}
              </Button>
              {settings.segmentationModelPath ? (
                <button
                  type="button"
                  className="panel-link panel-link--inline"
                  onClick={() => settings.update({ segmentationModelPath: null })}
                >
                  清除
                </button>
              ) : null}
            </div>
            <p className="field-hint">
              {mode === "tauri"
                ? "首次进入图层面板会用 512×512 空图实跑一次，验证输入与输出契约；不匹配会直接给出原因，不会生成空蒙版。"
                : "浏览器演示模式没有本地推理能力，智能蒙版仅在桌面版可用。"}
            </p>
          </section>

          <section className="settings-section">
            <div className="settings-section__head settings-section__head--stacked">
              <div>
                <h3>
                  <ScanFace size={14} aria-hidden="true" />
                  人脸识别模型
                </h3>
                <p>当前使用随应用打包、完全本地运行的 ONNX 人脸模型。</p>
              </div>
            </div>
            <div className="settings-model-list">
              <div className="settings-model-row">
                <span className="settings-model-row__name">CenterFace + YuNet</span>
                <code>ONNX · 本地</code>
                <span className={cn("badge", appInfo?.faceModelAvailable ? "badge--green" : "badge--amber")}>
                  {appInfo?.faceModelAvailable ? "已启用" : "不可用"}
                </span>
              </div>
            </div>
            <p className="field-hint">
              两个内置模型均为 MIT 许可；CenterFace 与 YuNet 会对同一张照片交叉检测，双模型一致的框合并为一。外部 ONNX / Python 模型尚未接入，避免引入未经验证的权重或任意脚本执行。
            </p>
          </section>

          <section className="settings-section settings-section--about">
            <div className="settings-section__head settings-section__head--stacked">
              <div>
                <h3>关于与本地数据</h3>
                <p>当前版本能力和数据位置，便于排查问题。</p>
              </div>
            </div>

            <dl className="settings-facts">
              <div>
                <dt>版本</dt>
                <dd>v{appInfo?.version ?? "0.1.0"}</dd>
              </div>
              <div>
                <dt>处理方式</dt>
                <dd>
                  {mode === "demo"
                    ? "浏览器演示 · 本地近似渲染"
                    : appInfo?.gpuAvailable
                      ? "桌面版 · GPU 可用（兼容文档自动加速，否则 CPU）"
                      : "桌面版 · CPU 本地渲染"}
                </dd>
              </div>
              <div className="settings-facts__path">
                <dt>工作区</dt>
                <dd>
                  <code title={appInfo?.workspaceDir ?? ""}>{appInfo?.workspaceDir ?? "—"}</code>
                  {mode === "tauri" && appInfo?.workspaceDir ? (
                    <button
                      type="button"
                      className="settings-icon-action"
                      aria-label="复制工作区路径"
                      title="复制工作区路径"
                      onClick={() => void handleCopyPath()}
                    >
                      <Copy size={11} />
                    </button>
                  ) : null}
                </dd>
              </div>
            </dl>

            <div className="settings-capabilities" aria-label="当前能力状态">
              {[
                {
                  key: "gpu",
                  available: appInfo?.gpuAvailable === true,
                  label: appInfo?.gpuAvailable === true ? "GPU 渲染可用" : "GPU 不可用",
                },
                {
                  key: "raw",
                  available: appInfo?.rawAvailable === true,
                  label: appInfo?.rawAvailable === true ? "RAW 已启用" : "RAW 未启用",
                },
                {
                  key: "face",
                  available: appInfo?.faceModelAvailable === true,
                  label:
                    appInfo?.faceModelAvailable === true
                      ? "人像识别模型已启用"
                      : "人像识别模型未接入",
                },
              ].map((capability) => (
                <span
                  key={capability.key}
                  className={cn("badge", capability.available ? "badge--green" : "badge--amber")}
                >
                  {capability.label}
                </span>
              ))}
            </div>
            {appInfo?.rawAvailable ? (
              <p className="field-hint">
                RAW 基础解码：松下 RW2/RWL、尼康 NEF/NRW、索尼 ARW；已用松下 DC-FZ80、尼康 1 J1、
                索尼 DSC-RX100 样片验证导入/预览/导出，其他机型取决于 rawler 相机数据库。
              </p>
            ) : null}
            {appInfo?.gpuAvailable ? (
              <p className="field-hint">
                GPU：调色预览/导出在兼容文档上使用 WGPU；人像图层或清晰度/锐度调整会回退 CPU，
                状态栏会显示当前实际后端。
              </p>
            ) : null}
            <p className="field-hint">
              编辑历史只保留在当前会话；关闭项目后，已提交的图层参数会保留，撤销记录不会跨重启恢复。
            </p>
          </section>
        </div>

        <footer className="export-dialog__foot settings-dialog__foot">
          <Button variant="ghost" size="sm" icon={<RotateCcw size={13} />} onClick={handleReset}>
            恢复默认设置
          </Button>
          <Button variant="primary" size="sm" onClick={close}>
            完成
          </Button>
        </footer>
      </div>
    </div>,
    document.body,
  );
}
