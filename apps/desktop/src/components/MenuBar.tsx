// Constraint brief: primary action remains "导出照片"; menu triggers are
// tertiary (muted text, no border), project switching is secondary. Spacing
// uses the existing 6/8/10px rhythm, 10–11px type and theme tokens
// (--surface-2/--raised/--accent-rgb), so light and dark stay consistent.
import { useEffect, useRef, useState } from "react";
import {
  Check,
  ChevronDown,
  Download,
  FolderOpen,
  Image as ImageIcon,
  Images,
  Palette,
  Pencil,
  Plus,
  Settings,
  Sparkles,
} from "lucide-react";
import type { CollectionSummary } from "../api/types";
import { resolveAssetUrl } from "../api/assets";
import { cn } from "../lib/cn";
import { formatRelativeTime } from "../lib/format";
import { isMacPlatform } from "../lib/shortcuts";
import { useEditorStore } from "../stores/editorStore";

type MenuId = "file" | "projects";

interface MenuCollectionRowProps {
  collection: CollectionSummary;
  current: boolean;
  onPick: () => void;
}

function MenuCollectionRow({
  collection,
  current,
  onPick,
}: MenuCollectionRowProps) {
  const thumb = resolveAssetUrl(collection.coverThumbnailPath);
  const renameCollection = useEditorStore((state) => state.renameCollection);
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState(collection.name);

  useEffect(() => {
    if (!editing) setName(collection.name);
  }, [collection.name, editing]);

  const commit = (): void => {
    const next = name.trim();
    setEditing(false);
    if (!next || next === collection.name) {
      setName(collection.name);
      return;
    }
    void renameCollection(collection.id, next);
  };

  if (editing) {
    return (
      <div className="menu-project__row is-editing">
        <input
          className="menu-project__rename-input"
          autoFocus
          value={name}
          maxLength={120}
          aria-label="照片集名称"
          onChange={(event) => setName(event.currentTarget.value)}
          onBlur={commit}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              commit();
            } else if (event.key === "Escape") {
              event.preventDefault();
              setName(collection.name);
              setEditing(false);
            }
          }}
        />
      </div>
    );
  }

  return (
    <div className="menu-project__row">
      <button type="button" className="menu-project" role="menuitem" onClick={onPick}>
        <span className="menu-project__thumb">
          {thumb ? <img src={thumb} alt="" /> : <ImageIcon size={13} />}
        </span>
        <span className="menu-project__copy">
          <span className="menu-project__name" title={collection.name}>
            {collection.name}
          </span>
          <span className="menu-project__meta">
            {collection.photoCount} 张照片 · {formatRelativeTime(collection.updatedAt)}
          </span>
        </span>
        {current ? <Check size={12} className="menu-project__check" aria-label="当前项目" /> : null}
      </button>
      <button
        type="button"
        className="menu-project__rename"
        aria-label={`重命名照片集 ${collection.name}`}
        title="重命名照片集"
        onClick={() => {
          setName(collection.name);
          setEditing(true);
        }}
      >
        <Pencil size={11} />
      </button>
    </div>
  );
}

export function MenuBar() {
  const [open, setOpen] = useState<MenuId | null>(null);
  const rootRef = useRef<HTMLDivElement>(null);
  const doc = useEditorStore((state) => state.doc);
  const collections = useEditorStore((state) => state.collections);
  const batchExport = useEditorStore((state) => state.batchExport);
  const selection = useEditorStore((state) => state.projectSelection);
  const sampleBusy = useEditorStore((state) => state.sampleBusy);
  const newProjectBusy = useEditorStore((state) => state.newProjectBusy);
  const importBusy = useEditorStore((state) => state.importBusy);
  const loadSample = useEditorStore((state) => state.loadSample);
  const openPhoto = useEditorStore((state) => state.openPhoto);
  const openCollection = useEditorStore((state) => state.openCollection);
  const openNewProject = useEditorStore((state) => state.openNewProject);
  const openExport = useEditorStore((state) => state.openExport);
  const openSettings = useEditorStore((state) => state.openSettings);
  const exportSelectedProjects = useEditorStore((state) => state.exportSelectedProjects);
  const syncAdjustmentsBusy = useEditorStore((state) => state.syncAdjustmentsBusy);
  const syncColorAdjustmentsToSelection = useEditorStore(
    (state) => state.syncColorAdjustmentsToSelection,
  );

  const mac = typeof navigator !== "undefined" && isMacPlatform(navigator.platform);
  const syncableSelection = selection.filter((projectId) => projectId !== doc?.id);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent): void => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(null);
    };
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key === "Escape") setOpen(null);
    };
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open]);

  const act = (action: () => void | Promise<void>): void => {
    setOpen(null);
    void action();
  };

  return (
    <div className="menubar" ref={rootRef}>
      <button
        type="button"
        className={cn("menubar__trigger", open === "file" && "is-open")}
        aria-haspopup="menu"
        aria-expanded={open === "file"}
        onClick={() => setOpen(open === "file" ? null : "file")}
      >
        文件
        <ChevronDown size={11} aria-hidden="true" />
      </button>
      {open === "file" ? (
        <div className="menu-pop" role="menu" aria-label="文件菜单">
          <p className="menu-pop__label">项目</p>
          <button
            type="button"
            role="menuitem"
            className="menu-item"
            disabled={newProjectBusy}
            onClick={() => act(openNewProject)}
          >
            <Plus size={14} aria-hidden="true" />
            <span>新建项目…</span>
            <kbd>{mac ? "⌘N" : "Ctrl+N"}</kbd>
          </button>
          <button
            type="button"
            role="menuitem"
            className="menu-item"
            disabled={sampleBusy}
            onClick={() => act(loadSample)}
          >
            <Sparkles size={14} aria-hidden="true" />
            <span>新建示例项目</span>
          </button>
          <button
            type="button"
            role="menuitem"
            className="menu-item"
            disabled={importBusy || newProjectBusy}
            title={doc ? "可一次选择多张，加入当前项目" : "可一次选择多张，创建新的照片集"}
            onClick={() => act(openPhoto)}
          >
            <FolderOpen size={14} aria-hidden="true" />
            <span>导入照片（可多选）…</span>
            <kbd>{mac ? "⌘O" : "Ctrl+O"}</kbd>
          </button>
          <div className="menu-pop__divider" role="separator" />
          <p className="menu-pop__label">导出</p>
          <button
            type="button"
            role="menuitem"
            className="menu-item"
            disabled={!doc?.source}
            onClick={() => act(openExport)}
          >
            <Download size={14} aria-hidden="true" />
            <span>导出当前照片…</span>
            <kbd>{mac ? "⌘S" : "Ctrl+S"}</kbd>
          </button>
          <button
            type="button"
            role="menuitem"
            className="menu-item"
            disabled={selection.length === 0 || Boolean(batchExport)}
            title={
              selection.length === 0
                ? "先在底部照片集勾选照片"
                : "按当前导出格式与质量，按各自裁剪后的原尺寸导出"
            }
            onClick={() => act(exportSelectedProjects)}
          >
            <Images size={14} aria-hidden="true" />
            <span>批量导出所选{selection.length > 0 ? `（${selection.length}）` : ""}…</span>
          </button>
          <button
            type="button"
            role="menuitem"
            className="menu-item"
            disabled={
              syncableSelection.length === 0 ||
              Boolean(batchExport) ||
              syncAdjustmentsBusy
            }
            title={
              syncableSelection.length === 0
                ? "先在底部照片集勾选当前照片以外的同集照片"
                : "复制当前照片的调色图层到所选照片，不改动原图"
            }
            onClick={() => act(syncColorAdjustmentsToSelection)}
          >
            <Palette size={14} aria-hidden="true" />
            <span>
              同步调色到所选
              {syncableSelection.length > 0 ? `（${syncableSelection.length}）` : ""}…
            </span>
          </button>
          <div className="menu-pop__divider" role="separator" />
          <button
            type="button"
            role="menuitem"
            className="menu-item"
            onClick={() => act(openSettings)}
          >
            <Settings size={14} aria-hidden="true" />
            <span>设置…</span>
            <kbd>{mac ? "⌘," : "Ctrl+,"}</kbd>
          </button>
        </div>
      ) : null}

      <button
        type="button"
        className={cn("menubar__trigger", open === "projects" && "is-open")}
        aria-haspopup="menu"
        aria-expanded={open === "projects"}
        onClick={() => setOpen(open === "projects" ? null : "projects")}
      >
        项目
        <ChevronDown size={11} aria-hidden="true" />
      </button>
      {open === "projects" ? (
        <div className="menu-pop menu-pop--projects" role="menu" aria-label="项目菜单">
          <p className="menu-pop__label">
            全部项目
            <span>{collections.length > 0 ? `${collections.length}` : "暂无"}</span>
          </p>
          {collections.length === 0 ? (
            <p className="menu-pop__empty">还没有项目，先新建一个照片集。</p>
          ) : (
            <div className="menu-pop__projects">
              {collections.map((collection) => (
                <MenuCollectionRow
                  key={collection.id}
                  collection={collection}
                  current={doc?.collectionId === collection.id}
                  onPick={() => act(() => openCollection(collection.id))}
                />
              ))}
            </div>
          )}
          <div className="menu-pop__divider" role="separator" />
          <button
            type="button"
            role="menuitem"
            className="menu-item"
            disabled={newProjectBusy}
            onClick={() => act(openNewProject)}
          >
            <Plus size={14} aria-hidden="true" />
            <span>新建项目…</span>
          </button>
          <button
            type="button"
            role="menuitem"
            className="menu-item"
            onClick={() => act(openSettings)}
          >
            <Settings size={14} aria-hidden="true" />
            <span>管理照片与缓存…</span>
          </button>
        </div>
      ) : null}
    </div>
  );
}
