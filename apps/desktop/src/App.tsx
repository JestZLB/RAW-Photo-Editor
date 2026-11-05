import { useEffect, useRef, useState } from "react";
import { ask } from "@tauri-apps/plugin-dialog";
import { ImagePlus } from "lucide-react";
import { isTauriRuntime } from "./api/assets";
import { isEditableTarget, resolveModifierShortcut, resolvePlainShortcut } from "./lib/shortcuts";
import { NewProjectDialog } from "./components/NewProjectDialog";
import { ProjectStrip } from "./components/ProjectStrip";
import { useEditorStore } from "./stores/editorStore";
import { useSettingsStore } from "./stores/settingsStore";
import { toastError } from "./stores/toastStore";
import { CanvasStage } from "./components/CanvasStage";
import { ExportDialog } from "./components/ExportDialog";
import { Inspector } from "./components/Inspector";
import { SettingsDialog } from "./components/SettingsDialog";
import { StatusBar } from "./components/StatusBar";
import { ToastViewport } from "./components/controls/Toast";
import { VersionsDialog } from "./components/VersionsDialog";
import { ToolRail } from "./components/ToolRail";
import { TopBar } from "./components/TopBar";
import { WelcomeScreen } from "./components/WelcomeScreen";
import { Spinner } from "./components/controls/Spinner";

export default function App() {
  const booted = useEditorStore((state) => state.booted);
  const doc = useEditorStore((state) => state.doc);
  const mode = useEditorStore((state) => state.mode);
  const api = useEditorStore((state) => state.api);
  const importFiles = useEditorStore((state) => state.importFiles);
  const reduceMotion = useSettingsStore((state) => state.reduceMotion);
  const theme = useSettingsStore((state) => state.theme);
  const [dragActive, setDragActive] = useState(false);
  const dragDepth = useRef(0);

  useEffect(() => {
    if (mode !== "tauri") return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    let closing = false;
    void (async () => {
      try {
        const { getCurrentWindow } = await import("@tauri-apps/api/window");
        const win = getCurrentWindow();
        const stop = await win.onCloseRequested(async (event) => {
          // Edits live in a 70ms debounce queue; without this the last slider
          // movement is lost when the window closes.
          event.preventDefault();
          if (closing) return;
          closing = true;
          let saved = true;
          try {
            const outcome = await Promise.race([
              useEditorStore.getState().flushPendingEdits(),
              new Promise<"timeout">((resolve) => setTimeout(() => resolve("timeout"), 3000)),
            ]);
            saved = outcome === true;
          } catch {
            saved = false;
          }
          if (!saved) {
            const closeAnyway = await ask("最近的调整没有保存成功。仍要关闭吗？", {
              title: "保存失败",
              kind: "warning",
              okLabel: "仍要关闭",
              cancelLabel: "返回",
            });
            if (!closeAnyway) {
              closing = false;
              return;
            }
          }
          await win.destroy();
        });
        if (disposed) stop();
        else unlisten = stop;
      } catch {
        // If the window API is unavailable, closing must still work.
      }
    })();
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [mode]);

  useEffect(() => {
    void useEditorStore.getState().bootstrap();
  }, []);

  useEffect(() => {
    document.documentElement.dataset.reduceMotion = reduceMotion ? "true" : "false";
  }, [reduceMotion]);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.defaultPrevented) return;
      const store = useEditorStore.getState();
      if (store.settingsOpen || store.exportOpen || store.versionsOpen) return;
      const shortcut = resolveModifierShortcut(event);
      if (shortcut) {
        if (isEditableTarget(event.target)) return;
        event.preventDefault();
        if (shortcut === "undo") void store.undo();
        else void store.redo();
        return;
      }
      if (event.ctrlKey || event.metaKey) {
        if (event.altKey || isEditableTarget(event.target)) return;
        const key = event.key.toLowerCase();
        if (key === "n") {
          event.preventDefault();
          store.openNewProject();
        } else if (key === "o") {
          event.preventDefault();
          void store.openPhoto();
        } else if (key === "s") {
          event.preventDefault();
          if (store.doc) store.openExport();
        } else if (key === ",") {
          event.preventDefault();
          store.openSettings();
        }
        return;
      }
      if (isEditableTarget(event.target) || !store.doc) return;
      const action = resolvePlainShortcut(event, {
        brushActive: store.maskBrushActive || store.tool === "retouch",
      });
      if (!action) return;
      if (action.type === "compare") {
        // Space must keep activating a focused button instead of comparing.
        if (event.target instanceof HTMLElement && /^(button|a|summary)$/i.test(event.target.tagName)) {
          return;
        }
        event.preventDefault();
        if (!event.repeat && store.doc.source) store.setComparing(true);
        return;
      }
      event.preventDefault();
      switch (action.type) {
        case "tool":
          store.setTool(action.tool);
          break;
        case "zoom":
          store.setZoom(store.zoom * (action.direction > 0 ? 1.25 : 1 / 1.25));
          break;
        case "fit":
          store.setZoom(1);
          break;
        case "delete-layer":
          if (store.selectedLayerId) void store.deleteLayer(store.selectedLayerId);
          break;
        case "brush-size": {
          const factor = action.direction > 0 ? 1.15 : 1 / 1.15;
          if (store.tool === "retouch" && !store.maskBrushActive) {
            store.setRetouchRadius(store.retouchRadius * factor);
          } else {
            store.setMaskBrushRadius(store.maskBrushRadius * factor);
          }
          break;
        }
        case "cancel":
          if (store.maskBrushActive) store.setMaskBrushActive(false);
          else if (store.whiteBalancePickActive) store.setWhiteBalancePickActive(false);
          break;
      }
    };
    const releaseCompare = (): void => {
      const store = useEditorStore.getState();
      if (store.comparing) store.setComparing(false);
    };
    const onKeyUp = (event: KeyboardEvent): void => {
      if (event.key === " ") releaseCompare();
    };
    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("keyup", onKeyUp);
    // A held Space must not stay "compared" after the window loses focus.
    window.addEventListener("blur", releaseCompare);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("keyup", onKeyUp);
      window.removeEventListener("blur", releaseCompare);
    };
  }, []);

  useEffect(() => {
    if (!api) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    let unlistenExport: (() => void) | undefined;
    void api
      .subscribeAiJobs((job) => {
        if (!disposed) useEditorStore.getState().receiveAiJob(job);
      })
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch(() => {
        // AI events are optional; the job list still refreshes on demand.
      });
    void api
      .subscribeExportProgress((progress) => {
        if (!disposed) useEditorStore.getState().receiveExportProgress(progress);
      })
      .then((stop) => {
        if (disposed) stop();
        else unlistenExport = stop;
      })
      .catch(() => {
        // The dialog still shows an indeterminate state without events.
      });
    return () => {
      disposed = true;
      unlisten?.();
      unlistenExport?.();
    };
  }, [api]);

  useEffect(() => {
    const hasFiles = (event: DragEvent): boolean => Array.from(event.dataTransfer?.types ?? []).includes("Files");
    const onDragEnter = (event: DragEvent): void => {
      if (!hasFiles(event)) return;
      event.preventDefault();
      dragDepth.current += 1;
      setDragActive(true);
    };
    const onDragOver = (event: DragEvent): void => {
      if (!hasFiles(event)) return;
      event.preventDefault();
      if (event.dataTransfer) event.dataTransfer.dropEffect = "copy";
    };
    const onDragLeave = (event: DragEvent): void => {
      if (!hasFiles(event)) return;
      dragDepth.current = Math.max(0, dragDepth.current - 1);
      if (dragDepth.current === 0) setDragActive(false);
    };
    const onDrop = (event: DragEvent): void => {
      event.preventDefault();
      dragDepth.current = 0;
      setDragActive(false);
      const files = event.dataTransfer?.files;
      if (!files || files.length === 0) {
        toastError("没有找到可以打开的照片，请拖入 JPG、PNG、TIFF 或 WebP 文件。");
        return;
      }
      void importFiles(Array.from(files));
    };
    window.addEventListener("dragenter", onDragEnter);
    window.addEventListener("dragover", onDragOver);
    window.addEventListener("dragleave", onDragLeave);
    window.addEventListener("drop", onDrop);
    return () => {
      window.removeEventListener("dragenter", onDragEnter);
      window.removeEventListener("dragover", onDragOver);
      window.removeEventListener("dragleave", onDragLeave);
      window.removeEventListener("drop", onDrop);
    };
  }, [importFiles]);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void import("@tauri-apps/api/webview")
      .then(({ getCurrentWebview }) =>
        getCurrentWebview().onDragDropEvent((event) => {
          if (disposed) return;
          const payload = event.payload;
          if (payload.type === "enter" || payload.type === "over") {
            setDragActive(true);
            return;
          }
          if (payload.type === "leave") {
            setDragActive(false);
            return;
          }
          setDragActive(false);
          if (payload.paths.length > 0) {
            void useEditorStore.getState().importPaths(payload.paths);
          } else {
            toastError("没有找到可以打开的照片，请拖入 JPG、PNG、TIFF 或 WebP 文件。");
          }
        }),
      )
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch(() => {
        // Native drag-and-drop is an enhancement; the file picker still works.
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  if (!booted) {
    return (
      <div className="boot-screen">
        <span className="boot-screen__mark">
          <img src="/app-icon.png" alt="" width={38} height={38} />
        </span>
        <p className="boot-screen__brand">Luma Studio</p>
        <span className="boot-screen__loading">
          <Spinner size={13} />
          正在准备本地工作区…
        </span>
      </div>
    );
  }

  return (
    <div className="app-shell">
      <TopBar />
      {doc ? (
        <>
          <div className="app-body">
            <ToolRail />
            <main className="workspace">
              <CanvasStage />
            </main>
            <Inspector />
          </div>
          <ProjectStrip />
        </>
      ) : (
        <WelcomeScreen />
      )}
      <StatusBar />
      <ExportDialog />
      <SettingsDialog />
      <NewProjectDialog />
      <VersionsDialog />
      <ToastViewport />
      {dragActive ? (
        <div className="drop-overlay" role="status" aria-live="polite">
          <div className="drop-overlay__card">
            <span className="drop-overlay__icon">
              <ImagePlus size={26} />
            </span>
            <p>松开以打开照片</p>
            <span>
              JPG · PNG · TIFF · WebP{mode === "tauri" ? " · RAW（最大 200 MB）" : "，最大 32 MB"}。
              原图不会被修改。
            </span>
          </div>
        </div>
      ) : null}
    </div>
  );
}
