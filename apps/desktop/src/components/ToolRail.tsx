import { ArrowUpFromLine, Crop, Image as ImageIcon, ScanFace, ShieldCheck, SlidersHorizontal, Stamp, WandSparkles } from "lucide-react";
import type { ToolId } from "../stores/editorStore";
import { useEditorStore } from "../stores/editorStore";

const TOOLS: Array<{ id: ToolId; label: string; icon: typeof ImageIcon }> = [
  { id: "photo", label: "照片", icon: ImageIcon },
  { id: "crop", label: "裁剪", icon: Crop },
  { id: "portrait", label: "人像", icon: ScanFace },
  { id: "color", label: "调色", icon: SlidersHorizontal },
  { id: "retouch", label: "修饰", icon: Stamp },
  { id: "ai", label: "AI", icon: WandSparkles },
];

export function ToolRail() {
  const tool = useEditorStore((state) => state.tool);
  const setTool = useEditorStore((state) => state.setTool);
  const openExport = useEditorStore((state) => state.openExport);

  return (
    <nav className="tool-rail" aria-label="编辑工具">
      <div className="tool-rail__items">
        {TOOLS.map((item, index) => {
          const Icon = item.icon;
          const active = tool === item.id;
          return (
            <button
              key={item.id}
              type="button"
              className={`tool ${active ? "is-active" : ""}`}
              aria-label={item.label}
              title={`${item.label}（${index + 1}）`}
              aria-current={active ? "page" : undefined}
              onClick={() => setTool(item.id)}
            >
              <span className="tool__icon">
                <Icon size={20} strokeWidth={1.7} />
              </span>
              <span className="tool__label">{item.label}</span>
            </button>
          );
        })}
        <div className="tool-rail__divider" />
        <button
          type="button"
          className="tool"
          aria-label="导出"
          title="导出照片（Ctrl / ⌘ S）"
          onClick={openExport}
        >
          <span className="tool__icon">
            <ArrowUpFromLine size={20} strokeWidth={1.7} />
          </span>
          <span className="tool__label">导出</span>
        </button>
      </div>
      <div className="tool-rail__foot" title="照片保存在本地，非破坏式编辑">
        <ShieldCheck size={14} aria-hidden="true" />
        <span>LOCAL</span>
      </div>
    </nav>
  );
}
