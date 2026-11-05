import { useEffect, useRef } from "react";
import { useEditorStore } from "../stores/editorStore";
import { AiPanel } from "../features/ai/AiPanel";
import { ColorPanel } from "../features/color/ColorPanel";
import { CropPanel } from "../features/crop/CropPanel";
import { PhotoPanel } from "../features/photo/PhotoPanel";
import { PortraitPanel } from "../features/portrait/PortraitPanel";
import { RetouchPanel } from "../features/retouch/RetouchPanel";
import { LayersPanel } from "./LayersPanel";

export function Inspector() {
  const tool = useEditorStore((state) => state.tool);
  const scrollRef = useRef<HTMLDivElement>(null);

  // Switching tools used to keep the previous scroll offset, which could drop
  // the user into the middle of a panel without its primary controls.
  useEffect(() => {
    scrollRef.current?.scrollTo({ top: 0 });
  }, [tool]);

  return (
    <aside className="inspector" aria-label="调整面板">
      <div className="inspector__scroll" ref={scrollRef}>
        <div className="panel-swap" key={tool}>
          {tool === "photo" ? <PhotoPanel /> : null}
          {tool === "crop" ? <CropPanel /> : null}
          {tool === "portrait" ? <PortraitPanel /> : null}
          {tool === "color" ? <ColorPanel /> : null}
          {tool === "ai" ? <AiPanel /> : null}
          {tool === "retouch" ? <RetouchPanel /> : null}
        </div>
      </div>
      <LayersPanel />
    </aside>
  );
}
