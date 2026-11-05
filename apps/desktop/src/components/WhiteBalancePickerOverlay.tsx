import { useEffect, type MouseEvent as ReactMouseEvent } from "react";
import { Pipette } from "lucide-react";
import { viewPointToSource } from "../lib/mask";
import { useEditorStore } from "../stores/editorStore";
import { Spinner } from "./controls/Spinner";

export function WhiteBalancePickerOverlay() {
  const doc = useEditorStore((state) => state.doc);
  const picking = useEditorStore((state) => state.whiteBalancePicking);
  const pickWhiteBalance = useEditorStore((state) => state.pickWhiteBalance);
  const setActive = useEditorStore((state) => state.setWhiteBalancePickActive);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key === "Escape") setActive(false);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [setActive]);

  if (!doc?.source || !doc.crop) return null;

  const sample = (event: ReactMouseEvent<HTMLButtonElement>): void => {
    if (picking) return;
    const rect = event.currentTarget.getBoundingClientRect();
    if (rect.width <= 0 || rect.height <= 0) return;
    const view = {
      x: (event.clientX - rect.left) / rect.width,
      y: (event.clientY - rect.top) / rect.height,
    };
    void pickWhiteBalance(
      viewPointToSource(view, doc.crop, doc.source!.width, doc.source!.height, doc.crop.angle ?? 0),
    );
  };

  return (
    <button
      type="button"
      className="white-balance-overlay"
      aria-label="点击画面中的中性灰区域校正白平衡，按 Esc 取消"
      disabled={picking}
      onPointerDown={(event) => event.stopPropagation()}
      onDoubleClick={(event) => event.stopPropagation()}
      onClick={sample}
    >
      <span className="white-balance-hint">
        {picking ? <Spinner size={12} /> : <Pipette size={13} aria-hidden="true" />}
        {picking ? "正在读取采样点…" : "点击画面中的中性灰区域 · Esc 取消"}
      </span>
    </button>
  );
}
