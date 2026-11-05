import { AlertTriangle, CheckCircle2, Info, X } from "lucide-react";
import { cn } from "../../lib/cn";
import { useToastStore } from "../../stores/toastStore";

const ICONS = {
  success: CheckCircle2,
  error: AlertTriangle,
  info: Info,
};

export function ToastViewport() {
  const toasts = useToastStore((state) => state.toasts);
  const dismiss = useToastStore((state) => state.dismiss);

  return (
    <div className="toast-viewport" aria-live="polite" aria-atomic="false">
      {toasts.map((toast) => {
        const Icon = ICONS[toast.kind];
        return (
          <div key={toast.id} className={cn("toast", `toast--${toast.kind}`)} role={toast.kind === "error" ? "alert" : "status"}>
            <span className="toast__icon">
              <Icon size={16} aria-hidden="true" />
            </span>
            <div className="toast__body">
              {toast.title ? <p className="toast__title">{toast.title}</p> : null}
              <p className="toast__message">{toast.message}</p>
            </div>
            <button type="button" className="toast__close" aria-label="关闭提示" title="关闭提示" onClick={() => dismiss(toast.id)}>
              <X size={13} />
            </button>
          </div>
        );
      })}
    </div>
  );
}
