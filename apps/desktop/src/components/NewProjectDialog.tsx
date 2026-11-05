import { useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { FolderOpen, Images, X } from "lucide-react";
import { useEditorStore } from "../stores/editorStore";
import { Button } from "./controls/Button";

export function NewProjectDialog() {
  const open = useEditorStore((state) => state.newProjectOpen);
  const busy = useEditorStore((state) => state.newProjectBusy);
  const close = useEditorStore((state) => state.closeNewProject);
  const createProjectFromPhoto = useEditorStore(
    (state) => state.createProjectFromPhoto,
  );
  const createEmptyProject = useEditorStore((state) => state.createEmptyProject);
  const [name, setName] = useState("");
  const dialogRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const titleId = useId();

  useEffect(() => {
    if (!open) return;
    setName("");
    const timer = window.setTimeout(() => inputRef.current?.focus(), 30);
    return () => window.clearTimeout(timer);
  }, [open]);

  if (!open) return null;

  const submit = async (): Promise<void> => {
    if (!name.trim() || busy) return;
    await createProjectFromPhoto(name);
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
        className="new-project-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            event.preventDefault();
            close();
          }
        }}
      >
        <header className="new-project-dialog__head">
          <div>
            <p className="new-project-dialog__eyebrow">NEW PROJECT</p>
            <h2 id={titleId}>新建项目</h2>
            <p>一个项目是一组照片。创建后可继续在底部照片集里添加照片。</p>
          </div>
          <button
            type="button"
            className="modal-close"
            aria-label="关闭新建项目窗口"
            disabled={busy}
            onClick={close}
          >
            <X size={14} />
          </button>
        </header>

        <div className="new-project-dialog__body">
          <label className="new-project-field">
            <span>项目名称</span>
            <input
              ref={inputRef}
              className="input"
              value={name}
              maxLength={120}
              placeholder="例如：婚礼精修、十月人像"
              aria-label="项目名称"
              disabled={busy}
              onChange={(event) => setName(event.currentTarget.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  void submit();
                }
              }}
            />
          </label>
          <p className="field-hint">
            可以直接选择第一张照片，也可以先创建空照片集，之后再导入一批照片。
          </p>
        </div>

        <footer className="new-project-dialog__foot">
          <Button variant="ghost" size="sm" disabled={busy} onClick={close}>
            取消
          </Button>
          <Button
            variant="secondary"
            size="sm"
            icon={<Images size={14} />}
            disabled={busy || !name.trim()}
            title="先创建照片集，稍后再导入照片"
            onClick={() => void createEmptyProject(name)}
          >
            只创建空照片集
          </Button>
          <Button
            variant="primary"
            size="sm"
            icon={<FolderOpen size={14} />}
            loading={busy}
            disabled={!name.trim()}
            onClick={() => void submit()}
          >
            选择照片并创建
          </Button>
        </footer>
      </div>
    </div>,
    document.body,
  );
}
