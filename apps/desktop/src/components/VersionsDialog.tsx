import { useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Eye, History, RotateCcw, Trash2, X } from "lucide-react";
import { resolveAssetUrl } from "../api/assets";
import { formatRelativeTime } from "../lib/format";
import { useEditorStore } from "../stores/editorStore";
import { Button } from "./controls/Button";
import { EmptyState } from "./controls/EmptyState";
import { Spinner } from "./controls/Spinner";

/**
 * Named snapshots. Unlike undo these live in SQLite, so they survive a restart;
 * restoring one is itself undoable.
 */
export function VersionsDialog() {
  const open = useEditorStore((state) => state.versionsOpen);
  const busy = useEditorStore((state) => state.versionsBusy);
  const versions = useEditorStore((state) => state.versions);
  const close = useEditorStore((state) => state.closeVersions);
  const createVersion = useEditorStore((state) => state.createVersion);
  const restoreVersion = useEditorStore((state) => state.restoreVersion);
  const deleteVersion = useEditorStore((state) => state.deleteVersion);
  const compare = useEditorStore((state) => state.versionCompare);
  const compareVersion = useEditorStore((state) => state.compareVersion);
  const clearVersionCompare = useEditorStore((state) => state.clearVersionCompare);
  const currentPreview = useEditorStore((state) => state.preview);
  const currentSource = useEditorStore((state) => state.doc?.source ?? null);
  const [name, setName] = useState("");
  const [confirmingId, setConfirmingId] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const titleId = useId();

  useEffect(() => {
    if (!open) {
      setName("");
      setConfirmingId(null);
      return;
    }
    const timer = window.setTimeout(() => inputRef.current?.focus(), 30);
    return () => window.clearTimeout(timer);
  }, [open]);

  if (!open) return null;

  const submit = async (): Promise<void> => {
    if (!name.trim() || busy) return;
    const created = await createVersion(name);
    if (created) setName("");
  };

  return createPortal(
    <div
      className="modal-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) close();
      }}
    >
      <div
        className="versions-dialog"
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
        <header className="versions-dialog__head">
          <div>
            <p className="versions-dialog__eyebrow">VERSIONS</p>
            <h2 id={titleId}>版本快照</h2>
            <p>把当前状态存成一个命名版本；重开项目后还能恢复，恢复本身也可以撤销。</p>
          </div>
          <button
            type="button"
            className="modal-close"
            aria-label="关闭版本窗口"
            disabled={busy}
            onClick={close}
          >
            <X size={14} />
          </button>
        </header>

        {compare ? (
          <div className="version-compare">
            <figure className="version-compare__pane">
              <span className="version-compare__tag">版本 · {compare.version.name}</span>
              <img
                src={resolveAssetUrl(compare.path) ?? ""}
                alt={`版本 ${compare.version.name} 的预览`}
              />
              <figcaption>
                {compare.width}×{compare.height} · 第 {compare.version.revision} 次修改
              </figcaption>
            </figure>
            <figure className="version-compare__pane">
              <span className="version-compare__tag">当前</span>
              <img
                src={resolveAssetUrl(currentPreview?.path ?? currentSource?.previewPath) ?? ""}
                alt="当前编辑状态"
              />
              <figcaption>
                {currentPreview
                  ? `${currentPreview.width}×${currentPreview.height}`
                  : "源图预览"}
              </figcaption>
            </figure>
          </div>
        ) : null}

        {compare ? (
          <p className="version-compare__hint">
            两侧各按自己的裁剪显示：比例不同，说明这个版本用过不同的裁剪或旋转。
          </p>
        ) : (
          <div className="versions-dialog__create">
            <input
              ref={inputRef}
              className="input"
              value={name}
              maxLength={60}
              placeholder="例如：定稿、暖调 v2"
              aria-label="版本名称"
              disabled={busy}
              onChange={(event) => setName(event.currentTarget.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  void submit();
                }
              }}
            />
            <Button
              variant="primary"
              size="sm"
              loading={busy}
              disabled={!name.trim()}
              onClick={() => void submit()}
            >
              保存当前状态
            </Button>
          </div>
        )}

        <div className="versions-dialog__list" hidden={compare !== null}>
          {busy && versions.length === 0 ? (
            <p className="versions-dialog__loading">
              <Spinner size={11} />
              正在读取版本…
            </p>
          ) : versions.length === 0 ? (
            <EmptyState
              icon={<History size={20} />}
              title="还没有保存过版本"
              description="存一个版本后，即使关掉应用也能回到这个状态。"
            />
          ) : (
            <ul>
              {versions.map((version) => {
                const thumb = resolveAssetUrl(version.thumbnailPath);
                const confirming = confirmingId === version.id;
                return (
                  <li key={version.id} className="version-row">
                    <span className="version-row__thumb">
                      {thumb ? <img src={thumb} alt="" /> : <History size={14} />}
                    </span>
                    <span className="version-row__copy">
                      <span className="version-row__name" title={version.name}>
                        {version.name}
                      </span>
                      <span className="version-row__meta">
                        第 {version.revision} 次修改 · {formatRelativeTime(version.createdAt)}
                      </span>
                    </span>
                    <span className="version-row__actions">
                      <Button
                        variant="ghost"
                        size="sm"
                        icon={<Eye size={12} />}
                        disabled={busy}
                        title="与当前状态并排对比"
                        onClick={() => void compareVersion(version.id)}
                      >
                        对比
                      </Button>
                      <Button
                        variant="secondary"
                        size="sm"
                        icon={<RotateCcw size={12} />}
                        disabled={busy}
                        title="恢复到这个版本，可用撤销回退"
                        onClick={() => void restoreVersion(version.id)}
                      >
                        恢复
                      </Button>
                      {confirming ? (
                        <Button
                          variant="secondary"
                          size="sm"
                          className="btn--danger"
                          disabled={busy}
                          onClick={() => {
                            setConfirmingId(null);
                            void deleteVersion(version.id);
                          }}
                        >
                          确认删除
                        </Button>
                      ) : (
                        <Button
                          variant="ghost"
                          size="sm"
                          icon={<Trash2 size={12} />}
                          disabled={busy}
                          aria-label={`删除版本 ${version.name}`}
                          title="删除这个版本"
                          onClick={() => setConfirmingId(version.id)}
                        >
                          删除
                        </Button>
                      )}
                    </span>
                  </li>
                );
              })}
            </ul>
          )}
        </div>

        <footer className="versions-dialog__foot">
          {compare ? (
            <>
              <Button variant="ghost" size="sm" disabled={busy} onClick={clearVersionCompare}>
                返回列表
              </Button>
              <Button
                variant="primary"
                size="sm"
                icon={<RotateCcw size={12} />}
                disabled={busy}
                onClick={() => void restoreVersion(compare.version.id)}
              >
                恢复这个版本
              </Button>
            </>
          ) : (
            <>
              <span>保存的是这个项目的图层、调色与人像参数；原图不会被改动。</span>
              <Button variant="ghost" size="sm" disabled={busy} onClick={close}>
                关闭
              </Button>
            </>
          )}
        </footer>
      </div>
    </div>,
    document.body,
  );
}
