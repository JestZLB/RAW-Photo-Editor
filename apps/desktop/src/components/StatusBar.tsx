import { AlertTriangle, Check, MonitorCheck } from "lucide-react";
import { formatDimensions, formatZoom } from "../lib/format";
import { useEditorStore } from "../stores/editorStore";
import { Spinner } from "./controls/Spinner";

export function StatusBar() {
  const doc = useEditorStore((state) => state.doc);
  const mode = useEditorStore((state) => state.mode);
  const appInfo = useEditorStore((state) => state.appInfo);
  const autosave = useEditorStore((state) => state.autosave);
  const retrySave = useEditorStore((state) => state.retrySave);
  const fitPercent = useEditorStore((state) => state.fitPercent);
  const zoom = useEditorStore((state) => state.zoom);
  const preview = useEditorStore((state) => state.preview);

  const zoomPercent = fitPercent > 0 ? fitPercent * zoom * 100 : zoom * 100;
  const dimensions = preview
    ? formatDimensions(preview.width, preview.height)
    : doc?.source
      ? formatDimensions(doc.source.width, doc.source.height)
      : "—";

  return (
    <footer className="statusbar">
      <div className="statusbar__left">
        {mode === "demo" ? (
          <>
            <span className="status-dot status-dot--amber" aria-hidden="true" />
            <span>演示模式 · 本地近似预览</span>
          </>
        ) : (
          <>
            <span className="status-dot status-dot--green" aria-hidden="true" />
            <MonitorCheck size={12} aria-hidden="true" />
            <span>
              本地处理 ·{" "}
              {preview?.backend === "wgpu"
                ? "GPU 渲染"
                : preview?.backend === "cache"
                  ? "预览缓存"
                  : preview?.backend === "source"
                    ? "源图预览"
                    : appInfo?.gpuAvailable
                      ? "CPU（当前文档回退）"
                      : "CPU"}
            </span>
          </>
        )}
      </div>
      <div className="statusbar__center">
        {doc ? (
          autosave === "saving" ? (
            <>
              <Spinner size={10} />
              <span>正在自动保存…</span>
            </>
          ) : autosave === "error" ? (
            <>
              <AlertTriangle size={12} aria-hidden="true" className="statusbar__error" />
              <span className="statusbar__error">保存失败，请检查磁盘空间后重试</span>
              <button
                type="button"
                className="statusbar__retry"
                onClick={() => void retrySave()}
              >
                重试
              </button>
            </>
          ) : (
            <>
              <Check size={12} aria-hidden="true" />
              <span>所有修改自动保存 · 原图不受影响</span>
            </>
          )
        ) : (
          <span>原图永不覆盖 · 所有调整都可以撤销</span>
        )}
      </div>
      <div className="statusbar__right">
        {doc ? (
          <>
            <span className="statusbar__metric">{dimensions}</span>
            <span className="statusbar__metric" title="当前显示比例">
              {formatZoom(zoomPercent)}
            </span>
            <span className="statusbar__metric" title="文档版本号">
              版本 {doc.revision}
            </span>
          </>
        ) : (
          <span className="statusbar__metric">v{appInfo?.version ?? "0.1.0"}</span>
        )}
      </div>
    </footer>
  );
}
