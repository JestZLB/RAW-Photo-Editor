import { create } from "zustand";

export type ToastKind = "success" | "error" | "info";

export interface ToastItem {
  id: number;
  kind: ToastKind;
  title: string | null;
  message: string;
}

interface ToastState {
  toasts: ToastItem[];
  push: (input: Omit<ToastItem, "id">) => number;
  dismiss: (id: number) => void;
}

const MAX_TOASTS = 3;
const AUTO_DISMISS_MS = 4000;

let nextToastId = 1;
const timers = new Map<number, ReturnType<typeof setTimeout>>();

function clearToastTimer(id: number): void {
  const timer = timers.get(id);
  if (timer !== undefined) {
    clearTimeout(timer);
    timers.delete(id);
  }
}

export const useToastStore = create<ToastState>()((set, get) => ({
  toasts: [],

  push: (input) => {
    const id = nextToastId++;
    const toast: ToastItem = { ...input, id };
    const existing = get().toasts;
    const next = [...existing, toast];
    const removed = next.length > MAX_TOASTS ? next.splice(0, next.length - MAX_TOASTS) : [];
    for (const item of removed) clearToastTimer(item.id);
    set({ toasts: next });
    const timer = setTimeout(() => {
      get().dismiss(id);
    }, AUTO_DISMISS_MS);
    timers.set(id, timer);
    return id;
  },

  dismiss: (id) => {
    clearToastTimer(id);
    set((state) => ({ toasts: state.toasts.filter((toast) => toast.id !== id) }));
  },
}));

export function toastSuccess(message: string, title = "已完成"): void {
  useToastStore.getState().push({ kind: "success", title, message });
}

export function toastError(message: string, title = "操作未完成"): void {
  useToastStore.getState().push({ kind: "error", title, message });
}

export function toastInfo(message: string, title: string | null = null): void {
  useToastStore.getState().push({ kind: "info", title, message });
}
