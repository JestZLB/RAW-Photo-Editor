import { useEffect, useRef, useState, type ReactNode } from "react";
import {
  Brush,
  ChevronDown,
  ChevronUp,
  CircleDot,
  Copy,
  CopyPlus,
  Eraser,
  Eye,
  EyeOff,
  FlipHorizontal2,
  Image as ImageIcon,
  Layers,
  Lock,
  MoveUpRight,
  Plus,
  ScanFace,
  SlidersHorizontal,
  Sparkles,
  Trash2,
} from "lucide-react";
import type { BlendMode, LayerKind, LayerMask, LayerState } from "../api/types";
import { cn } from "../lib/cn";
import { canMoveLayer, layersTopFirst } from "../lib/layers";
import { defaultMaskGradient, maskHasContent, maskSummary } from "../lib/mask";
import { useEditorStore } from "../stores/editorStore";
import { Button } from "./controls/Button";
import { Slider } from "./controls/Slider";

const BLEND_OPTIONS: Array<{ value: BlendMode; label: string }> = [
  { value: "normal", label: "正常" },
  { value: "multiply", label: "正片叠底" },
  { value: "screen", label: "滤色" },
  { value: "overlay", label: "叠加" },
  { value: "softLight", label: "柔光" },
];

interface MaskOptionProps {
  mask: LayerMask;
  onApply: (patch: Partial<Pick<LayerMask, "feather" | "density">>) => void;
}

function MaskOptions({ mask, onApply }: MaskOptionProps) {
  const [feather, setFeather] = useState(Math.round((mask.feather ?? 0) * 100));
  const [density, setDensity] = useState(Math.round((mask.density ?? 1) * 100));
  const featherRef = useRef(feather);
  const densityRef = useRef(density);

  useEffect(() => {
    const value = Math.round((mask.feather ?? 0) * 100);
    featherRef.current = value;
    setFeather(value);
  }, [mask.feather]);
  useEffect(() => {
    const value = Math.round((mask.density ?? 1) * 100);
    densityRef.current = value;
    setDensity(value);
  }, [mask.density]);

  return (
    <div className="layer-mask__options">
      <Slider
        label="边缘羽化"
        value={feather}
        min={0}
        max={100}
        step={1}
        defaultValue={0}
        unit="%"
        compact
        showReset={false}
        onChange={(value) => {
          featherRef.current = value;
          setFeather(value);
        }}
        onCommit={() => onApply({ feather: featherRef.current / 100 })}
      />
      <Slider
        label="修饰浓度"
        value={density}
        min={0}
        max={100}
        step={1}
        defaultValue={100}
        unit="%"
        compact
        showReset={false}
        onChange={(value) => {
          densityRef.current = value;
          setDensity(value);
        }}
        onCommit={() => onApply({ density: densityRef.current / 100 })}
      />
    </div>
  );
}

function KindIcon({ kind }: { kind: LayerKind }) {
  switch (kind) {
    case "base":
      return <ImageIcon size={13} />;
    case "portrait":
      return <ScanFace size={13} />;
    case "color":
      return <SlidersHorizontal size={13} />;
    case "aiResult":
      return <Sparkles size={13} />;
  }
}

export function LayersPanel() {
  const doc = useEditorStore((state) => state.doc);
  const selectedLayerId = useEditorStore((state) => state.selectedLayerId);
  const selectLayer = useEditorStore((state) => state.selectLayer);
  const addLayer = useEditorStore((state) => state.addLayer);
  const deleteLayer = useEditorStore((state) => state.deleteLayer);
  const duplicateLayer = useEditorStore((state) => state.duplicateLayer);
  const copyLayerAdjustments = useEditorStore((state) => state.copyLayerAdjustments);
  const moveLayer = useEditorStore((state) => state.moveLayer);
  const setLayerVisibility = useEditorStore((state) => state.setLayerVisibility);
  const setLayerBlendMode = useEditorStore((state) => state.setLayerBlendMode);
  const setLayerOpacity = useEditorStore((state) => state.setLayerOpacity);
  const commitEdit = useEditorStore((state) => state.commitEdit);
  const maskBrushActive = useEditorStore((state) => state.maskBrushActive);
  const maskBrushRadius = useEditorStore((state) => state.maskBrushRadius);
  const maskBrushErase = useEditorStore((state) => state.maskBrushErase);
  const setMaskBrushActive = useEditorStore((state) => state.setMaskBrushActive);
  const setMaskBrushRadius = useEditorStore((state) => state.setMaskBrushRadius);
  const setMaskBrushErase = useEditorStore((state) => state.setMaskBrushErase);
  const setAutoSkinMask = useEditorStore((state) => state.setAutoSkinMask);
  const setMaskGradient = useEditorStore((state) => state.setMaskGradient);
  const setMaskOptions = useEditorStore((state) => state.setMaskOptions);
  const clearLayerMask = useEditorStore((state) => state.clearLayerMask);

  const [menuOpen, setMenuOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!menuOpen) return;
    const onPointerDown = (event: PointerEvent): void => {
      if (menuRef.current && event.target instanceof Node && !menuRef.current.contains(event.target)) {
        setMenuOpen(false);
      }
    };
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key === "Escape") setMenuOpen(false);
    };
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [menuOpen]);

  if (!doc) return null;

  const rows = layersTopFirst(doc.layers);
  const base = doc.layers.find((layer) => layer.kind === "base") ?? null;
  const layerCount = doc.layers.length;

  const handleAdd = (kind: "portrait" | "color"): void => {
    setMenuOpen(false);
    void addLayer(kind);
  };

  const renderDetail = (layer: LayerState): ReactNode => {
    if (layer.id !== selectedLayerId) return null;
    if (layer.kind === "base") {
      return (
        <div className="layer-detail layer-detail--base">
          <Lock size={11} aria-hidden="true" />
          <span>原始照片只读保存，任何调整都不会写回原图。</span>
        </div>
      );
    }
    return (
      <div className="layer-detail">
        <Slider
          label="不透明度"
          value={Math.round(layer.opacity * 100)}
          min={0}
          max={100}
          step={1}
          defaultValue={100}
          unit="%"
          compact
          showReset={false}
          disabled={layer.locked}
          onChange={(value) => setLayerOpacity(layer.id, value / 100)}
          onCommit={() => void commitEdit("调整图层不透明度")}
        />
        <div className="layer-detail__row">
          <label className="layer-detail__blend">
            <span>混合方式</span>
            <span className="select-wrap select-wrap--mini">
              <select
                className="select"
                value={layer.blendMode}
                disabled={layer.locked}
                aria-label="混合方式"
                onChange={(event) => void setLayerBlendMode(layer.id, event.currentTarget.value as BlendMode)}
              >
                {BLEND_OPTIONS.map((option) => (
                  <option key={option.value} value={option.value}>
                    {option.label}
                  </option>
                ))}
              </select>
              <ChevronDown size={11} className="select-wrap__icon" aria-hidden="true" />
            </span>
          </label>
          <div className="layer-detail__actions">
            <button
              type="button"
              className="mini-action"
              aria-label="上移图层"
              title="上移图层"
              disabled={!canMoveLayer(doc, layer, "up")}
              onClick={() => void moveLayer(layer.id, "up")}
            >
              <ChevronUp size={13} />
            </button>
            <button
              type="button"
              className="mini-action"
              aria-label="下移图层"
              title="下移图层"
              disabled={!canMoveLayer(doc, layer, "down")}
              onClick={() => void moveLayer(layer.id, "down")}
            >
              <ChevronDown size={13} />
            </button>
            <button
              type="button"
              className="mini-action"
              aria-label="复制图层"
              title="复制图层（参数与修饰范围一并复制）"
              onClick={() => void duplicateLayer(layer.id)}
            >
              <CopyPlus size={13} />
            </button>
            {layer.kind === "color" || layer.kind === "portrait" ? (
              <button
                type="button"
                className="mini-action"
                aria-label="复制参数到其他照片"
                title="复制参数，可打开其他照片后粘贴"
                onClick={() => copyLayerAdjustments(layer.id)}
              >
                <Copy size={13} />
              </button>
            ) : null}
            <button
              type="button"
              className="mini-action mini-action--danger"
              aria-label="删除图层"
              title="删除图层（可用撤销恢复）"
              disabled={layer.locked}
              onClick={() => void deleteLayer(layer.id)}
            >
              <Trash2 size={13} />
            </button>
          </div>
        </div>
        {layer.kind === "color" || layer.kind === "portrait" ? (
          <div className={cn("layer-mask", maskBrushActive && "is-active")}>
            <div className="layer-mask__head">
              <span>局部修饰</span>
              <span className="layer-mask__status">{maskSummary(layer.mask)}</span>
            </div>
            <div className="layer-mask__actions">
              <Button
                variant={maskBrushActive ? "primary" : "secondary"}
                size="sm"
                icon={maskBrushErase ? <Eraser size={13} /> : <Brush size={13} />}
                onClick={() => setMaskBrushActive(!maskBrushActive)}
              >
                {maskBrushActive ? "结束画笔" : "局部画笔"}
              </Button>
              <button
                type="button"
                className={cn(
                  "mini-action layer-mask__auto",
                  layer.mask?.autoSkin && "is-active",
                )}
                aria-label={layer.mask?.autoSkin ? "关闭自动肤色范围" : "启用自动肤色范围"}
                title="按肤色范围自动生成修饰区域（启发式，不是 AI 语义分割）"
                aria-pressed={layer.mask?.autoSkin === true}
                onClick={() => void setAutoSkinMask(layer.id, !layer.mask?.autoSkin)}
              >
                <Sparkles size={12} />
                <span>自动肤色</span>
              </button>
              <button
                type="button"
                className={cn("mini-action layer-mask__mode", maskBrushErase && "is-active")}
                aria-label={maskBrushErase ? "切回涂抹" : "切换为擦除"}
                title={maskBrushErase ? "当前为擦除，点击切回涂抹" : "擦除已涂抹的区域"}
                aria-pressed={maskBrushErase}
                onClick={() => setMaskBrushErase(!maskBrushErase)}
              >
                <Eraser size={13} />
                <span>{maskBrushErase ? "擦除中" : "擦除"}</span>
              </button>
              {layer.mask ? (
                <button
                  type="button"
                  className="mini-action mini-action--danger layer-mask__clear"
                  aria-label="清除修饰范围"
                  title="清除这个图层的全部修饰范围"
                  onClick={() => void clearLayerMask(layer.id)}
                >
                  清除
                </button>
              ) : null}
            </div>
            <div className="layer-mask__gradients">
              <button
                type="button"
                className={cn(
                  "mini-action layer-mask__gradient",
                  layer.mask?.gradient?.kind === "linear" && "is-active",
                )}
                aria-pressed={layer.mask?.gradient?.kind === "linear"}
                title="添加线性渐变范围；再次点击移除"
                onClick={() =>
                  void setMaskGradient(
                    layer.id,
                    layer.mask?.gradient?.kind === "linear"
                      ? null
                      : defaultMaskGradient("linear"),
                  )
                }
              >
                <MoveUpRight size={12} />
                <span>线性</span>
              </button>
              <button
                type="button"
                className={cn(
                  "mini-action layer-mask__gradient",
                  layer.mask?.gradient?.kind === "radial" && "is-active",
                )}
                aria-pressed={layer.mask?.gradient?.kind === "radial"}
                title="添加径向渐变范围；再次点击移除"
                onClick={() =>
                  void setMaskGradient(
                    layer.id,
                    layer.mask?.gradient?.kind === "radial"
                      ? null
                      : defaultMaskGradient("radial"),
                  )
                }
              >
                <CircleDot size={12} />
                <span>径向</span>
              </button>
              <button
                type="button"
                className={cn(
                  "mini-action layer-mask__invert",
                  layer.mask?.inverted && "is-active",
                )}
                aria-pressed={layer.mask?.inverted === true}
                disabled={!layer.mask}
                title="反转修饰范围"
                onClick={() =>
                  void setMaskOptions(layer.id, { inverted: !layer.mask?.inverted })
                }
              >
                <FlipHorizontal2 size={12} />
                <span>反转</span>
              </button>
            </div>
            {layer.mask && maskHasContent(layer.mask) ? (
              <MaskOptions
                mask={layer.mask}
                onApply={(patch) => void setMaskOptions(layer.id, patch)}
              />
            ) : null}
            {maskBrushActive ? (
              <Slider
                label="画笔大小"
                value={Math.round(maskBrushRadius * 100)}
                min={1}
                max={20}
                step={1}
                defaultValue={6}
                unit="%"
                compact
                showReset={false}
                onChange={(value) => setMaskBrushRadius(value / 100)}
              />
            ) : null}
            <p className="layer-mask__hint">
              {maskBrushActive
                ? "在画布上涂抹；只影响当前图层的调整。带蒙版时预览会回退 CPU。[ / ] 调整画笔大小，Esc 退出。"
                : layer.mask?.gradient
                  ? "拖动画面中的两个端点调整渐变方向与范围；渐变可与画笔、自动肤色叠加。"
                  : "渐变用于局部过渡；自动肤色按肤色范围选取（启发式，不是 AI 分割）。"}
            </p>
          </div>
        ) : null}
      </div>
    );
  };

  return (
    <section className="layers-dock" aria-label="图层">
      <header className="layers-dock__head">
        <div className="layers-dock__title">
          <Layers size={13} aria-hidden="true" />
          <span>图层</span>
          <span className="layers-dock__count">{layerCount}</span>
        </div>
        <div className="layers-add" ref={menuRef}>
          <button
            type="button"
            className={cn("mini-action", menuOpen && "is-active")}
            aria-label="新建调整图层"
            title="新建调整图层"
            aria-haspopup="menu"
            aria-expanded={menuOpen}
            onClick={() => setMenuOpen((open) => !open)}
          >
            <Plus size={14} />
          </button>
          {menuOpen ? (
            <div className="layers-add__menu" role="menu" aria-label="新建调整图层">
              <button type="button" role="menuitem" onClick={() => handleAdd("portrait")}>
                <span className="layers-add__icon layers-add__icon--portrait">
                  <ScanFace size={14} />
                </span>
                <span>
                  <strong>人像调整</strong>
                  <small>磨皮、肤色与提亮</small>
                </span>
              </button>
              <button type="button" role="menuitem" onClick={() => handleAdd("color")}>
                <span className="layers-add__icon layers-add__icon--color">
                  <SlidersHorizontal size={14} />
                </span>
                <span>
                  <strong>调色调整</strong>
                  <small>光线、颜色与细节</small>
                </span>
              </button>
            </div>
          ) : null}
        </div>
      </header>

      <div className="layers-dock__list">
        {rows.length === 0 && !base ? (
          <p className="layers-dock__empty">还没有图层。打开照片后可新建调整图层。</p>
        ) : null}

        {rows.map((layer) => (
          <div key={layer.id} className={cn("layer-item", layer.id === selectedLayerId && "is-selected")}>
            <div className="layer-row">
              <button
                type="button"
                className="layer-eye"
                aria-label={layer.visible ? `隐藏${layer.name}` : `显示${layer.name}`}
                title={layer.visible ? "隐藏图层" : "显示图层"}
                disabled={layer.locked}
                onClick={() => void setLayerVisibility(layer.id, !layer.visible)}
              >
                {layer.visible ? <Eye size={13} /> : <EyeOff size={13} />}
              </button>
              <button
                type="button"
                className="layer-select"
                aria-label={`选择图层 ${layer.name}`}
                aria-current={layer.id === selectedLayerId ? "true" : undefined}
                onClick={() => selectLayer(layer.id)}
              >
                <span className={cn("layer-kind-icon", `layer-kind-icon--${layer.kind}`)}>
                  <KindIcon kind={layer.kind} />
                </span>
                <span className="layer-name">{layer.name}</span>
                {layer.locked ? <Lock size={10} className="layer-lock" aria-label="已锁定" /> : null}
                <span className="layer-opacity">{Math.round(layer.opacity * 100)}%</span>
              </button>
            </div>
            {renderDetail(layer)}
          </div>
        ))}

        {base ? (
          <div className={cn("layer-item layer-item--base", base.id === selectedLayerId && "is-selected")}>
            <div className="layer-row">
              <span className="layer-eye layer-eye--locked" title="原始照片只读">
                <Lock size={12} />
              </span>
              <button
                type="button"
                className="layer-select"
                aria-label="选择原始照片图层"
                onClick={() => selectLayer(base.id)}
              >
                <span className="layer-kind-icon layer-kind-icon--base">
                  <KindIcon kind="base" />
                </span>
                <span className="layer-name">{base.name}</span>
                <span className="layer-pin">只读</span>
              </button>
            </div>
            {renderDetail(base)}
          </div>
        ) : null}
      </div>
    </section>
  );
}
