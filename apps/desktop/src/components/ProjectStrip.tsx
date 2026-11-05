// Constraint brief: primary here is "批量导出" when a selection exists;
// switching projects is secondary, select/clear/collapse are tertiary.
// Values come from existing tokens (--surface, --raised, --accent-rgb,
// 8–10px radii, 9–11.5px type) and the app's 6/8/10px spacing rhythm.
import { useState } from "react";
import {
  Check,
  ChevronDown,
  ChevronUp,
  Download,
  Image as ImageIcon,
  Images,
  Palette,
  Plus,
} from "lucide-react";
import type { ProjectSummary } from "../api/types";
import { resolveAssetUrl } from "../api/assets";
import { cn } from "../lib/cn";
import { formatDimensions, formatRelativeTime } from "../lib/format";
import { useEditorStore } from "../stores/editorStore";
import { Button, IconButton } from "./controls/Button";
import { Spinner } from "./controls/Spinner";

interface ProjectCardProps {
  project: ProjectSummary;
  current: boolean;
  selected: boolean;
  disabled: boolean;
  onOpen: () => void;
  onToggle: () => void;
}

function ProjectCard({
  project,
  current,
  selected,
  disabled,
  onOpen,
  onToggle,
}: ProjectCardProps) {
  const thumb = resolveAssetUrl(project.thumbnailPath);
  return (
    <div
      className={cn(
        "project-card",
        current && "is-current",
        selected && "is-selected",
      )}
    >
      <button
        type="button"
        className="project-card__open"
        disabled={disabled}
        title={current ? `当前项目：${project.name}` : `打开 ${project.name}`}
        aria-current={current ? "page" : undefined}
        onClick={onOpen}
      >
        <span className="project-card__thumb">
          {thumb ? <img src={thumb} alt="" /> : <ImageIcon size={14} />}
        </span>
        <span className="project-card__copy">
          <span className="project-card__title-row">
            <span className="project-card__name" title={project.name}>
              {project.name}
            </span>
            {current ? <span className="project-card__current">当前</span> : null}
          </span>
          <span className="project-card__meta">
            {formatDimensions(project.width, project.height)} ·{" "}
            {formatRelativeTime(project.updatedAt)}
          </span>
        </span>
      </button>
      <button
        type="button"
        className="project-card__check"
        aria-label={selected ? `取消选择 ${project.name}` : `选择 ${project.name} 用于批量导出`}
        aria-pressed={selected}
        title={selected ? "取消选择" : "选择用于批量导出"}
        onClick={onToggle}
      >
        {selected ? <Check size={10} strokeWidth={3} /> : null}
      </button>
    </div>
  );
}

export function ProjectStrip() {
  const [open, setOpen] = useState(true);
  const doc = useEditorStore((state) => state.doc);
  const recent = useEditorStore((state) => state.recent);
  const selection = useEditorStore((state) => state.projectSelection);
  const batchExport = useEditorStore((state) => state.batchExport);
  const importBatch = useEditorStore((state) => state.importBatch);
  const syncBusy = useEditorStore((state) => state.syncAdjustmentsBusy);
  const importBusy = useEditorStore((state) => state.importBusy);
  const openProject = useEditorStore((state) => state.openProject);
  const openPhoto = useEditorStore((state) => state.openPhoto);
  const toggleProjectSelection = useEditorStore((state) => state.toggleProjectSelection);
  const selectAllProjects = useEditorStore((state) => state.selectAllProjects);
  const clearProjectSelection = useEditorStore((state) => state.clearProjectSelection);
  const exportSelectedProjects = useEditorStore((state) => state.exportSelectedProjects);
  const syncColorAdjustmentsToSelection = useEditorStore(
    (state) => state.syncColorAdjustmentsToSelection,
  );

  const collectionId = doc?.collectionId;
  const photos = collectionId
    ? recent.filter(
        (project) =>
          project.collectionId === collectionId &&
          project.width > 0 &&
          project.height > 0,
      )
    : [];
  const selectedCount = photos.filter((project) => selection.includes(project.id)).length;
  const syncableCount = photos.filter(
    (project) => selection.includes(project.id) && project.id !== doc?.id,
  ).length;
  const exporting = Boolean(batchExport);
  const importing = Boolean(importBatch);
  const busy = exporting || syncBusy || importing;
  const allSelected = photos.length > 0 && selectedCount === photos.length;
  const progress = batchExport
    ? Math.round(((batchExport.done + batchExport.failed) / Math.max(1, batchExport.total)) * 100)
    : importBatch
      ? Math.round((importBatch.done / Math.max(1, importBatch.total)) * 100)
      : 0;

  return (
    <section className={cn("project-strip", !open && "is-collapsed")} aria-label="项目照片集">
      <div className="project-strip__head">
        <div className="project-strip__title">
          <Images size={14} aria-hidden="true" />
          <strong>{doc?.collectionName || "照片集"}</strong>
          <span className="project-strip__count">{photos.length}</span>
          <span className="project-strip__hint">
            {!open
              ? selectedCount > 0
                ? `已选 ${selectedCount} 个`
                : ""
              : batchExport
                ? `正在导出 ${batchExport.done + batchExport.failed}/${batchExport.total} · ${batchExport.currentName}`
                : importBatch
                  ? `正在导入 ${importBatch.done}/${importBatch.total} · ${importBatch.currentName}`
                  : syncBusy
                    ? "正在同步当前调色…"
                    : selectedCount > 0
                      ? `已选 ${selectedCount} 个，可批量导出或同步调色`
                      : "勾选照片后批量导出，或把当前调色同步过去"}
          </span>
        </div>

        {open ? (
          <div className="project-strip__actions">
            {importBatch ? (
              <>
                <span className="project-strip__progress-label">导入中</span>
                <Spinner size={12} />
                <span className="project-strip__progress-value">{progress}%</span>
              </>
            ) : batchExport ? (
              <>
                <span className="project-strip__progress-label">导出中</span>
                <Spinner size={12} />
                <span className="project-strip__progress-value">{progress}%</span>
              </>
            ) : (
              <>
                <button
                  type="button"
                  className="panel-link panel-link--inline"
                  disabled={photos.length === 0 || busy}
                  onClick={allSelected ? clearProjectSelection : selectAllProjects}
                >
                  {allSelected ? "取消全选" : "全选"}
                </button>
                {selectedCount > 0 ? (
                  <button
                    type="button"
                    className="panel-link panel-link--inline"
                    disabled={busy}
                    onClick={clearProjectSelection}
                  >
                    清空
                  </button>
                ) : null}
                <Button
                  variant="secondary"
                  size="sm"
                  icon={<Download size={13} />}
                  disabled={selectedCount === 0 || importBusy || busy}
                  title="按当前导出格式与质量，按各自裁剪后的原尺寸导出"
                  onClick={() => void exportSelectedProjects()}
                >
                  批量导出
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  icon={<Palette size={13} />}
                  disabled={syncableCount === 0 || importBusy || busy}
                  title={
                    syncableCount === 0
                      ? "先勾选当前照片以外的同集照片"
                      : `把当前照片的调色图层复制到 ${syncableCount} 张所选照片`
                  }
                  onClick={() => void syncColorAdjustmentsToSelection()}
                >
                  同步调色
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  icon={<Plus size={13} />}
                  disabled={importBusy || busy}
                  title="一次选择多张照片，加入当前项目"
                  onClick={() => void openPhoto()}
                >
                  添加照片
                </Button>
              </>
            )}
            <IconButton
              label={open ? "收起项目集" : "展开项目集"}
              icon={open ? <ChevronDown size={14} /> : <ChevronUp size={14} />}
              size="sm"
              onClick={() => setOpen((value) => !value)}
            />
          </div>
        ) : (
          <div className="project-strip__actions">
            <IconButton
              label="展开项目集"
              icon={<ChevronUp size={14} />}
              size="sm"
              onClick={() => setOpen(true)}
            />
          </div>
        )}
      </div>

      {open ? (
        <>
          {photos.length === 0 ? (
            <p className="project-strip__empty">
              这个项目还没有照片。点击「添加照片」可一次导入多张，之后可在这里切换、同步调色与批量导出。
            </p>
          ) : (
            <div className="project-strip__list">
              {photos.map((project) => (
                <ProjectCard
                  key={project.id}
                  project={project}
                  current={doc?.id === project.id}
                  selected={selection.includes(project.id)}
                  disabled={importBusy || busy}
                  onOpen={() => void openProject(project.id)}
                  onToggle={() => toggleProjectSelection(project.id)}
                />
              ))}
            </div>
          )}
          {importBatch || batchExport ? (
            <div className="project-strip__bar" aria-hidden="true">
              <span style={{ width: `${progress}%` }} />
            </div>
          ) : null}
        </>
      ) : null}
    </section>
  );
}
