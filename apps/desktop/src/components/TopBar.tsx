import { Columns2, Download, Eye, Plus, Redo2, Settings, Undo2 } from "lucide-react";
import { isMacPlatform, shortcutHint } from "../lib/shortcuts";
import { useEditorStore } from "../stores/editorStore";
import { Button, IconButton } from "./controls/Button";
import { MenuBar } from "./MenuBar";

export function TopBar() {
  const doc = useEditorStore((state) => state.doc);
  const autosave = useEditorStore((state) => state.autosave);
  const comparing = useEditorStore((state) => state.comparing);
  const undo = useEditorStore((state) => state.undo);
  const redo = useEditorStore((state) => state.redo);
  const setComparing = useEditorStore((state) => state.setComparing);
  const splitCompare = useEditorStore((state) => state.splitCompare);
  const setSplitCompare = useEditorStore((state) => state.setSplitCompare);
  const openExport = useEditorStore((state) => state.openExport);
  const openSettings = useEditorStore((state) => state.openSettings);
  const openNewProject = useEditorStore((state) => state.openNewProject);

  const mac = typeof navigator !== "undefined" && isMacPlatform(navigator.platform);
  const hints = shortcutHint(mac);
  const historyLabels = doc?.history.slice(-4).reverse() ?? [];
  const hasSource = Boolean(doc?.source);

  const startCompare = (): void => {
    if (hasSource) setComparing(true);
  };
  const endCompare = (): void => {
    if (comparing) setComparing(false);
  };

  return (
    <header className="topbar">
      <div className="topbar__left">
        <div className="brand">
          <span className="brand__mark">
            <img src="/app-icon.png" alt="" width={26} height={26} />
          </span>
          <span className="brand__name">Luma</span>
          <span className="brand__badge">STUDIO</span>
        </div>
        <span className="topbar__divider" aria-hidden="true" />
        <MenuBar />
        <span className="topbar__divider" aria-hidden="true" />
        <Button
          variant="secondary"
          size="sm"
          icon={<Plus size={14} />}
          className="topbar-new"
          onClick={openNewProject}
        >
          新建项目
        </Button>
        <IconButton
          label={`设置（${mac ? "⌘" : "Ctrl"} ,）`}
          icon={<Settings size={16} />}
          onClick={openSettings}
        />
      </div>

      <div className="topbar__center">
        {doc ? (
          <div
            className="project-title"
            title={`${doc.collectionName || doc.name} / ${doc.name}`}
          >
            <span className="project-title__name">{doc.collectionName || doc.name}</span>
            {doc.collectionName && doc.collectionName !== doc.name ? (
              <>
                <span className="project-title__sep" aria-hidden="true">
                  /
                </span>
                <span className="project-title__photo" title={doc.name}>
                  {doc.name}
                </span>
              </>
            ) : null}
            <span
              className={`save-dot ${
                autosave === "saving" ? "is-saving" : autosave === "error" ? "is-error" : "is-saved"
              }`}
              aria-hidden="true"
            />
            <span className="project-title__state">
              {autosave === "saving" ? "正在保存" : autosave === "error" ? "保存失败" : "已自动保存"}
            </span>
          </div>
        ) : (
          <div className="project-title project-title--empty">
            <span className="project-title__name">本地人像工作室</span>
            <span className="project-title__state">照片只留在你的电脑上</span>
          </div>
        )}
      </div>

      <div className="topbar__right">
        <div className="history-actions">
          <div className="history-anchor">
            <IconButton
              label={doc?.canUndo ? `撤销（${hints.undo}）` : "暂无可撤销的操作"}
              icon={<Undo2 size={16} />}
              disabled={!doc?.canUndo}
              onClick={() => void undo()}
            />
            <div className="history-pop" role="tooltip">
              <p className="history-pop__head">最近操作</p>
              {historyLabels.length > 0 ? (
                <ul>
                  {historyLabels.map((label, index) => (
                    <li key={`${label}-${index}`}>
                      <span className="history-pop__dot" aria-hidden="true" />
                      {label}
                    </li>
                  ))}
                </ul>
              ) : (
                <p className="history-pop__empty">还没有可撤销的调整</p>
              )}
              <p className="history-pop__foot">{hints.undo} 撤销</p>
            </div>
          </div>
          <div className="history-anchor">
            <IconButton
              label={doc?.canRedo ? `重做（${hints.redo}）` : "暂无可重做的操作"}
              icon={<Redo2 size={16} />}
              disabled={!doc?.canRedo}
              onClick={() => void redo()}
            />
            <div className="history-pop" role="tooltip">
              <p className="history-pop__head">重做</p>
              <p className="history-pop__empty">{doc?.canRedo ? `按 ${hints.redo} 恢复下一步。` : "还没有可重做的调整"}</p>
            </div>
          </div>
        </div>

        <IconButton
          label={splitCompare ? "关闭分屏对比" : "分屏对比原图"}
          icon={<Columns2 size={16} />}
          active={splitCompare}
          disabled={!hasSource}
          onClick={() => setSplitCompare(!splitCompare)}
        />

        <Button
          variant="secondary"
          size="sm"
          icon={<Eye size={14} />}
          disabled={!hasSource}
          className={`compare-btn ${comparing ? "is-active" : ""}`}
          title="按住查看未修改的原图（空格）"
          aria-pressed={comparing}
          onPointerDown={(event) => {
            if (!hasSource) return;
            event.currentTarget.setPointerCapture(event.pointerId);
            startCompare();
          }}
          onPointerUp={endCompare}
          onPointerCancel={endCompare}
          onPointerLeave={endCompare}
          onKeyDown={(event) => {
            if (event.key === " " || event.key === "Enter") {
              event.preventDefault();
              startCompare();
            }
          }}
          onKeyUp={(event) => {
            if (event.key === " " || event.key === "Enter") {
              event.preventDefault();
              endCompare();
            }
          }}
          onBlur={endCompare}
        >
          按住对比
        </Button>

        <Button
          variant="primary"
          size="sm"
          icon={<Download size={14} />}
          disabled={!hasSource}
          onClick={openExport}
        >
          导出照片
        </Button>

      </div>
    </header>
  );
}
