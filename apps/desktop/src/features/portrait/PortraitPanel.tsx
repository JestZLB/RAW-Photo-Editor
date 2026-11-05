import { useEffect, useState } from "react";
import {
  ClipboardPaste,
  Copy,
  Droplets,
  Feather,
  Leaf,
  LockKeyhole,
  Move,
  RotateCcw,
  ScanFace,
  Sparkles,
  Star,
  Sun,
  X,
} from "lucide-react";
import type { PortraitFace, PortraitParams, PortraitRegion } from "../../api/types";
import { cn } from "../../lib/cn";
import { selectedOrKind } from "../../lib/layers";
import { MASKABLE_REGIONS, maskHasContent } from "../../lib/mask";
import {
  BASE_PORTRAIT_EFFECTS,
  BEAUTY_PORTRAIT_EFFECTS,
  BODY_PORTRAIT_EFFECTS,
  DEFAULT_PORTRAIT,
  enabledPortraitEffects,
  FINE_PORTRAIT_EFFECTS,
  PORTRAIT_EFFECTS,
  PORTRAIT_PRESETS,
  WRINKLE_ZONE_EFFECTS,
  type PortraitEffectKey,
  type PortraitPresetId,
} from "../../lib/params";
import { useEditorStore } from "../../stores/editorStore";
import { PanelHeader } from "../../components/PanelHeader";
import { Button, IconButton } from "../../components/controls/Button";
import { Collapsible } from "../../components/controls/Collapsible";
import { Segmented } from "../../components/controls/Segmented";
import { PanelSection } from "../../components/controls/PanelSection";
import { Slider } from "../../components/controls/Slider";
import { Spinner } from "../../components/controls/Spinner";
import { Toggle } from "../../components/controls/Toggle";
import { FacePicker } from "./FacePicker";

const PRESET_ICONS = {
  natural: Leaf,
  clear: Sun,
  soft: Feather,
  creamy: Droplets,
  oxygen: Sparkles,
  stage: Star,
} as const;

const INTENSITY_OPTIONS = [
  { value: "55", label: "轻一点", title: "更克制，几乎看不出调整" },
  { value: "78", label: "适中", title: "日常照片的稳妥选择" },
  { value: "100", label: "明显", title: "效果更突出" },
];

type PortraitSliderKey =
  | "smooth"
  | "smoothRadius"
  | "texture"
  | "evenSkin"
  | "brighten"
  | "intensity";

/** These effects need an eye/mouth position; defect repair can use skin alone. */
const NEEDS_FACE_GEOMETRY = new Set<PortraitEffectKey>([
  "eyeEnhance",
  "teethWhiten",
  "darkCircle",
  "eyeBag",
  "wrinkle",
  "wrinkleForehead",
  "wrinkleGlabella",
  "wrinkleCrowFeet",
  "wrinkleNasolabial",
  "wrinkleMouthCorner",
  "wrinkleUnderEye",
  "wrinkleNeck",
]);

const ADVANCED_CONTROLS: Array<{
  key: PortraitSliderKey;
  label: string;
  hint: string;
}> = [
  { key: "smooth", label: "磨皮", hint: "柔化肤质，同时保留光影层次" },
  { key: "smoothRadius", label: "磨皮半径", hint: "半径越大覆盖越广；0 为自动" },
  { key: "texture", label: "保留皮肤细节", hint: "数值越高，皮肤纹理保留越多" },
  { key: "evenSkin", label: "肤色均匀", hint: "减轻局部泛红与色差" },
  { key: "brighten", label: "自然提亮", hint: "让肤色更透亮" },
  { key: "intensity", label: "整体程度", hint: "统一放大或收敛以上效果" },
];

type ShapeKey =
  | "faceSlim"
  | "jawWidth"
  | "chinLength"
  | "eyeDistance"
  | "eyeSize"
  | "noseSlim";

const SHAPE_CONTROLS: Array<{
  key: ShapeKey;
  label: string;
  hint: string;
  min: number;
  max: number;
}> = [
  { key: "faceSlim", label: "瘦脸", hint: "收窄脸颊与脸缘", min: 0, max: 100 },
  { key: "jawWidth", label: "下颌线", hint: "集中收窄下颌与下巴两侧", min: 0, max: 100 },
  {
    key: "chinLength",
    label: "下巴长度",
    hint: "负值缩短、正值拉长下巴",
    min: -100,
    max: 100,
  },
  {
    key: "eyeDistance",
    label: "眼距",
    hint: "负值拉近、正值拉开双眼",
    min: -100,
    max: 100,
  },
  { key: "eyeSize", label: "大眼", hint: "放大眼睛及眼周", min: 0, max: 100 },
  { key: "noseSlim", label: "瘦鼻", hint: "收窄鼻翼，保持鼻梁位置", min: 0, max: 100 },
];

const SKIN_ADVANCED_CONTROLS: Array<{
  key: "whitenSaturation" | "oilControl";
  label: string;
  hint: string;
}> = [
  {
    key: "whitenSaturation",
    label: "美白通透",
    hint: "额外降低肤色饱和度，减少黄气与厚重感",
  },
  { key: "oilControl", label: "去油光", hint: "压暗皮肤上的高光油亮区域" },
];

const SKIN_SPOT_KEYS = ["blemish", "mole", "freckle", "birthmark", "tattoo"] as const;
const SKIN_SPOT_EFFECTS = FINE_PORTRAIT_EFFECTS.filter((effect) =>
  (SKIN_SPOT_KEYS as readonly string[]).includes(effect.key),
);
const FIVE_DETAIL_EFFECTS = FINE_PORTRAIT_EFFECTS.filter(
  (effect) => !(SKIN_SPOT_KEYS as readonly string[]).includes(effect.key),
);

type NumericParamKey =
  | "whitenSaturation"
  | "oilControl"
  | "eyeWhite"
  | "irisDetail"
  | "teethBrighten"
  | "eyeshadow"
  | "browTint"
  | "lipTint"
  | "noseContour";

type PortraitTab = "skin" | "five" | "makeup" | "shape" | "body" | "advanced";

const PORTRAIT_TABS: Array<{ id: PortraitTab; label: string }> = [
  { id: "skin", label: "皮肤" },
  { id: "five", label: "五官" },
  { id: "makeup", label: "妆容" },
  { id: "shape", label: "脸型" },
  { id: "body", label: "身体" },
  { id: "advanced", label: "高级" },
];

const FIVE_DETAIL_CONTROLS: Array<{
  key: "eyeWhite" | "irisDetail" | "teethBrighten";
  label: string;
  hint: string;
}> = [
  { key: "eyeWhite", label: "眼白提亮", hint: "只提亮眼白，保留虹膜与瞳孔" },
  { key: "irisDetail", label: "虹膜细节", hint: "增强虹膜纹理与眼神光" },
  { key: "teethBrighten", label: "牙齿亮度", hint: "在去黄之外单独提亮牙齿" },
];

const MAKEUP_CONTROLS: Array<{
  key: "eyeshadow" | "browTint" | "lipTint" | "noseContour";
  label: string;
  hint: string;
}> = [
  { key: "eyeshadow", label: "眼影", hint: "在上眼睑加入暖棕眼影，保留睫毛与眼球" },
  { key: "browTint", label: "眉色", hint: "加深并温暖眉毛，让五官更立体" },
  { key: "lipTint", label: "唇色", hint: "在唇部区域加入自然血色" },
  { key: "noseContour", label: "鼻影", hint: "加深鼻翼两侧，轻轻提亮鼻梁" },
];

function matchesPreset(params: PortraitParams, presetId: PortraitPresetId): boolean {
  const preset = PORTRAIT_PRESETS.find((item) => item.id === presetId);
  if (!preset) return false;
  return (Object.keys(preset.settings) as Array<keyof PortraitParams>).every((key) => {
    if (key === "faceIndex" || key === "geometry" || key === "bodyMode") return true;
    const current = params[key];
    const target = preset.settings[key];
    return (
      typeof current === "number" &&
      typeof target === "number" &&
      Math.abs(current - target) < 0.01
    );
  });
}

export function PortraitPanel() {
  const doc = useEditorStore((state) => state.doc);
  const selectedLayerId = useEditorStore((state) => state.selectedLayerId);
  const applyPreset = useEditorStore((state) => state.applyPortraitPreset);
  const commitPortrait = useEditorStore((state) => state.commitPortrait);
  const editPortrait = useEditorStore((state) => state.editPortrait);
  const commitEdit = useEditorStore((state) => state.commitEdit);
  const setPortraitEffect = useEditorStore((state) => state.setPortraitEffect);
  const resetPortrait = useEditorStore((state) => state.resetPortrait);
  const copiedAdjustments = useEditorStore((state) => state.copiedAdjustments);
  const copyLayerAdjustments = useEditorStore((state) => state.copyLayerAdjustments);
  const pasteLayerAdjustments = useEditorStore((state) => state.pasteLayerAdjustments);

  const [applying, setApplying] = useState<PortraitPresetId | null>(null);
  const [tab, setTab] = useState<PortraitTab>("skin");

  // A tab switch should start at the first control, not inherit the previous
  // tab's scroll offset.
  useEffect(() => {
    document.querySelector(".inspector__scroll")?.scrollTo({ top: 0 });
  }, [tab]);

  const api = useEditorStore((state) => state.api);
  const mode = useEditorStore((state) => state.mode);
  const faceAnalysis = useEditorStore((state) => state.faceAnalysis);
  const faceLayerBusy = useEditorStore((state) => state.faceLayerBusy);
  const selectedFaceIndex = useEditorStore((state) => state.selectedFaceIndex);
  const ensureFacePortraitLayer = useEditorStore((state) => state.ensureFacePortraitLayer);
  const ensureBodyPortraitLayer = useEditorStore((state) => state.ensureBodyPortraitLayer);
  const selectLayer = useEditorStore((state) => state.selectLayer);
  const faceRegions = useEditorStore((state) => state.faceRegions);
  const regionBusy = useEditorStore((state) => state.regionBusy);
  const regionOverlayActive = useEditorStore((state) => state.regionOverlayActive);
  const analyzeFaceRegions = useEditorStore((state) => state.analyzeFaceRegions);
  const setRegionOverlayActive = useEditorStore((state) => state.setRegionOverlayActive);
  const geometryEditActive = useEditorStore((state) => state.geometryEditActive);
  const setGeometryEditActive = useEditorStore((state) => state.setGeometryEditActive);
  const addMaskRegion = useEditorStore((state) => state.addMaskRegion);
  const removeMaskRegion = useEditorStore((state) => state.removeMaskRegion);
  const setTool = useEditorStore((state) => state.setTool);

  const faces = faceAnalysis?.faces ?? [];
  const activeFace: PortraitFace | null = faces[selectedFaceIndex] ?? null;
  const selectedPortrait = selectedOrKind(doc, selectedLayerId, "portrait");
  const faceSpecificLayer =
    doc?.layers.find(
      (item) => item.kind === "portrait" && item.portrait?.faceIndex === selectedFaceIndex,
    ) ?? null;
  const selectablePortrait =
    selectedPortrait && selectedPortrait.portrait?.bodyMode !== true
      ? selectedPortrait
      : null;
  const layer = faceSpecificLayer ?? (faces.length <= 1 ? selectablePortrait : null);
  const params = layer?.portrait ?? DEFAULT_PORTRAIT;
  const bodyLayer =
    doc?.layers.find((item) => item.kind === "portrait" && item.portrait?.bodyMode) ?? null;
  const bodyParams = bodyLayer?.portrait ?? DEFAULT_PORTRAIT;
  const tabState: Record<PortraitTab, boolean> = {
    skin:
      params.smooth > 0 ||
      params.evenSkin > 0 ||
      params.brighten > 0 ||
      params.whiten > 0 ||
      params.rosy > 0 ||
      SKIN_SPOT_EFFECTS.some((effect) => params[effect.key] > 0),
    five:
      params.eyeEnhance > 0 ||
      params.teethWhiten > 0 ||
      params.darkCircle > 0 ||
      params.eyeBag > 0 ||
      params.wrinkle > 0 ||
      params.eyeWhite > 0 ||
      params.irisDetail > 0 ||
      params.teethBrighten > 0 ||
      params.noseContour > 0 ||
      params.lipTint > 0 ||
      WRINKLE_ZONE_EFFECTS.some((effect) => params[effect.key] > 0),
    makeup:
      params.eyeshadow > 0 ||
      params.browTint > 0 ||
      params.lipTint > 0 ||
      params.noseContour > 0,
    shape: SHAPE_CONTROLS.some((control) => params[control.key] !== 0),
    body: bodyLayer
      ? BODY_PORTRAIT_EFFECTS.some((effect) => bodyParams[effect.key] > 0)
      : false,
    advanced: false,
  };
  const enabled = enabledPortraitEffects(params);
  const activeFaceRegions = activeFace
    ? faceRegions.find((item) => item.faceId === activeFace.faceId) ?? null
    : faceRegions[0] ?? null;
  const colorLayer = selectedOrKind(doc, selectedLayerId, "color");
  const maskOwner = layer ?? colorLayer;
  const maskRegionKinds = (maskOwner?.mask?.regions ?? []).map((region) => region.kind);
  const maskRegionIndexes: Record<string, number> = {};
  (maskOwner?.mask?.regions ?? []).forEach((region, index) => {
    maskRegionIndexes[region.kind] = index;
  });
  const regionLayerId = maskOwner?.id ?? null;

  const handlePreset = async (presetId: PortraitPresetId): Promise<void> => {
    setApplying(presetId);
    try {
      await applyPreset(presetId);
    } finally {
      setApplying(null);
    }
  };

  const handleIntensity = (value: string): void => {
    if (!layer) return;
    const intensity = Number(value);
    void commitPortrait(layer.id, { ...params, intensity }, "调整美颜程度");
  };

  const handleEffectToggle = async (
    key: PortraitEffectKey,
    checked: boolean,
  ): Promise<void> => {
    let target = layer;
    if (!target && activeFace) {
      const effect = PORTRAIT_EFFECTS.find((item) => item.key === key);
      target = await ensureFacePortraitLayer(
        effect ? { [key]: checked ? effect.onValue : 0 } : undefined,
      );
      if (!target) return;
      selectLayer(target.id);
      // A newly created face layer already carries the requested effect.
      if (target.portrait?.faceIndex === selectedFaceIndex) return;
    }
    if (!target) return;
    selectLayer(target.id);
    await setPortraitEffect(target.id, key, checked);
  };

  const handleRegionChip = async (region: PortraitRegion): Promise<void> => {
    let target = layer;
    if (!target && activeFace) target = await ensureFacePortraitLayer();
    if (!target) return;
    selectLayer(target.id);
    await addMaskRegion({
      kind: region.kind,
      centerX: region.centerX,
      centerY: region.centerY,
      radiusX: region.radiusX,
      radiusY: region.radiusY,
      angle: region.angle,
      feather: region.feather,
      strength: 1,
    });
  };

  const handleParamChange = async (
    key: NumericParamKey,
    value: number,
  ): Promise<void> => {
    let target = layer;
    if (!target && activeFace) {
      target = await ensureFacePortraitLayer({ [key]: value });
      if (!target) return;
      selectLayer(target.id);
      if (target.portrait?.[key] === value) return;
    }
    if (!target?.portrait) return;
    selectLayer(target.id);
    editPortrait(target.id, { ...target.portrait, [key]: value });
  };

  const handleShapeChange = async (key: ShapeKey, value: number): Promise<void> => {
    let target = layer;
    if (!target && activeFace) {
      target = await ensureFacePortraitLayer({ [key]: value });
      if (!target) return;
      selectLayer(target.id);
      if (target.portrait?.[key] === value) return;
    }
    if (!target?.portrait) return;
    selectLayer(target.id);
    editPortrait(target.id, { ...target.portrait, [key]: value });
  };

  const handleBodyShapeChange = async (
    key: "bodySlim" | "bodyLengthen",
    value: number,
  ): Promise<void> => {
    let target = bodyLayer;
    if (!target) {
      target = await ensureBodyPortraitLayer();
      if (!target) return;
    }
    const latest = useEditorStore
      .getState()
      .doc?.layers.find((item) => item.id === target?.id);
    if (!latest?.portrait) return;
    selectLayer(latest.id);
    editPortrait(latest.id, { ...latest.portrait, [key]: value });
  };

  const handleBodyEffectToggle = async (
    key: PortraitEffectKey,
    checked: boolean,
  ): Promise<void> => {
    let target = bodyLayer;
    if (!target) {
      if (!checked) return;
      target = await ensureBodyPortraitLayer();
      if (!target) return;
    }
    selectLayer(target.id);
    await setPortraitEffect(target.id, key, checked);
  };

  const renderEffectRow = (effect: (typeof PORTRAIT_EFFECTS)[number]) => (
    <div key={effect.key} className="fine-effect-row">
      <Toggle
        label={effect.name}
        description={effect.caption}
        checked={params[effect.key] > 0}
        disabled={
          (!layer && !activeFace) ||
          faceLayerBusy ||
          (!params.geometry &&
            !activeFace &&
            params[effect.key] <= 0 &&
            NEEDS_FACE_GEOMETRY.has(effect.key))
        }
        onChange={(checked) => {
          void handleEffectToggle(effect.key, checked);
        }}
      />
      {layer && params[effect.key] > 0 ? (
        <Slider
          label={`${effect.name}强度`}
          value={params[effect.key]}
          min={0}
          max={100}
          step={1}
          defaultValue={effect.onValue}
          unit="%"
          onChange={(value) => {
            editPortrait(layer.id, { ...params, [effect.key]: value });
          }}
          onCommit={() => void commitEdit(`微调${effect.name}`)}
        />
      ) : null}
    </div>
  );

  return (
    <div className="panel portrait-panel">
      <div className="portrait-sticky">
        <PanelHeader
          icon={<ScanFace size={18} />}
          title="人像精修"
          description="先点一个效果，再按感觉微调"
          action={
            layer || copiedAdjustments?.kind === "portrait" ? (
              <div className="panel-head-actions">
                {layer ? (
                  <IconButton
                    label="复制人像参数"
                    icon={<Copy size={14} />}
                    size="sm"
                    onClick={() => copyLayerAdjustments(layer.id)}
                  />
                ) : null}
                {copiedAdjustments?.kind === "portrait" ? (
                  <IconButton
                    label="粘贴人像参数"
                    icon={<ClipboardPaste size={14} />}
                    size="sm"
                    onClick={() => void pasteLayerAdjustments()}
                  />
                ) : null}
                {layer ? (
                  <IconButton
                    label="恢复人像默认"
                    icon={<RotateCcw size={14} />}
                    size="sm"
                    onClick={() => void resetPortrait(layer.id)}
                  />
                ) : null}
              </div>
            ) : undefined
          }
        />
        <FacePicker />
        <nav className="portrait-tabs" role="tablist" aria-label="人像调整分区">
          {PORTRAIT_TABS.map((item) => (
            <button
              key={item.id}
              type="button"
              role="tab"
              aria-selected={tab === item.id}
              className={cn("portrait-tab", tab === item.id && "is-active")}
              onClick={() => setTab(item.id)}
            >
              {item.label}
              {tabState[item.id] ? (
                <span className="portrait-tab__dot" aria-hidden="true" />
              ) : null}
            </button>
          ))}
        </nav>
      </div>

      {tab === "skin" ? (
        <>
          <section className="panel-section">
            <div className="section-heading">
              <span>快速风格</span>
              <span className="section-note">默认就很自然</span>
            </div>
            <div className="style-chips" role="group" aria-label="快速风格">
              {PORTRAIT_PRESETS.map((preset) => {
                const Icon = PRESET_ICONS[preset.id];
                const active = Boolean(layer) && matchesPreset(params, preset.id);
                return (
                  <button
                    key={preset.id}
                    type="button"
                    className={cn("style-chip", active && "is-active")}
                    aria-pressed={active}
                    title={`${preset.name}：${preset.description}`}
                    onClick={() => void handlePreset(preset.id)}
                  >
                    <span className={cn("style-chip__icon", `style-chip__icon--${preset.id}`)}>
                      {applying === preset.id ? <Spinner size={12} /> : <Icon size={13} />}
                    </span>
                    {preset.name}
                  </button>
                );
              })}
            </div>
            {!layer ? (
              <p className="inline-note">
                <Sparkles size={12} aria-hidden="true" />
                {activeFace
                  ? `当前是${faces.length > 1 ? `人脸 ${selectedFaceIndex + 1}` : "这张人脸"}，点选风格或打开效果会自动创建只作用于这张脸的美颜层。`
                  : "还没有人像调整图层。选择一种风格，会新建图层并保留原图。"}
              </p>
            ) : layer.portrait?.faceIndex != null ? (
              <p className="inline-note">
                <Sparkles size={12} aria-hidden="true" />
                当前图层只作用于人脸 {layer.portrait.faceIndex + 1}，其他人物不受影响。
              </p>
            ) : null}
          </section>

      <PanelSection title="整体程度" hint="影响所有已开启的效果">
        <Segmented
          value={layer ? String(Math.round(params.intensity)) : null}
          options={INTENSITY_OPTIONS}
          onChange={handleIntensity}
          ariaLabel="美颜整体程度"
          disabled={!layer}
        />
      </PanelSection>

      <PanelSection title="效果开关" hint={layer ? "随时关掉" : activeFace ? "开关会新建此脸美颜层" : "先选一种风格"}>
        <div className="switch-list">
          {BASE_PORTRAIT_EFFECTS.map((effect) => (
            <Toggle
              key={effect.key}
              label={effect.name}
              description={effect.caption}
              checked={params[effect.key] > 0}
              disabled={(!layer && !activeFace) || faceLayerBusy}
              onChange={(checked) => {
                void handleEffectToggle(effect.key, checked);
              }}
            />
          ))}
        </div>

        {layer ? (
          enabled.length > 0 ? (
            <div className="effect-pills" aria-label="当前启用的效果">
              {enabled.map((effect) => (
                <button
                  key={effect.key}
                  type="button"
                  className="effect-pill"
                  title={`关闭${effect.name}`}
                  onClick={() => void setPortraitEffect(layer.id, effect.key, false)}
                >
                  {effect.name}
                  <X size={10} aria-hidden="true" />
                </button>
              ))}
            </div>
          ) : (
            <p className="effect-pills__empty">还没有启用效果，画面与原图一致。</p>
          )
        ) : null}
      </PanelSection>

      <PanelSection title="肤色美颜" hint="美白、气色与眼袋阴影">
        <div className="switch-list">
          {BEAUTY_PORTRAIT_EFFECTS.map((effect) => (
            <div key={effect.key} className="fine-effect-row">
              <Toggle
                label={effect.name}
                description={effect.caption}
                checked={params[effect.key] > 0}
                disabled={
                  (!layer && !activeFace) ||
                  faceLayerBusy ||
                  (!params.geometry &&
                    !activeFace &&
                    params[effect.key] <= 0 &&
                    NEEDS_FACE_GEOMETRY.has(effect.key))
                }
                onChange={(checked) => {
                  void handleEffectToggle(effect.key, checked);
                }}
              />
              {layer && params[effect.key] > 0 ? (
                <Slider
                  label={`${effect.name}强度`}
                  value={params[effect.key]}
                  min={0}
                  max={100}
                  step={1}
                  defaultValue={effect.onValue}
                  unit="%"
                  onChange={(value) => {
                    editPortrait(layer.id, { ...params, [effect.key]: value });
                  }}
                  onCommit={() => void commitEdit(`微调${effect.name}`)}
                />
              ) : null}
            </div>
          ))}
        </div>
        {layer && (params.whiten > 0 || params.whitenWarmth !== 0) ? (
          <Slider
            label="美白冷暖"
            hint="负值更冷，正值更暖"
            value={params.whitenWarmth}
            min={-100}
            max={100}
            step={1}
            defaultValue={0}
            onChange={(value) => {
              editPortrait(layer.id, { ...params, whitenWarmth: value });
            }}
            onCommit={() => void commitEdit("调整美白冷暖")}
          />
        ) : null}
        <Collapsible
          title="高级肤色"
          subtitle="通透、油光与范围"
          defaultOpen={false}
          trailing={<span className="collapsible__badge">细分</span>}
        >
          {SKIN_ADVANCED_CONTROLS.map((control) => (
            <Slider
              key={control.key}
              label={control.label}
              hint={control.hint}
              value={params[control.key]}
              min={0}
              max={100}
              step={1}
              defaultValue={0}
              unit="%"
              disabled={!layer && !activeFace}
              onChange={(value) => {
                void handleParamChange(control.key, value);
              }}
              onCommit={() => void commitEdit(control.label)}
            />
          ))}
        </Collapsible>
        <p className="inline-note">
          <Sparkles size={12} aria-hidden="true" />
          美白只做视觉提亮与黄气减轻，不承诺改变真实肤色；眼袋只处理袋状阴影，不做结构重塑。
        </p>
      </PanelSection>

      <PanelSection title="瑕疵与色斑" hint="痘印、痣、雀斑、胎记、纹身">
        <div className="switch-list">{SKIN_SPOT_EFFECTS.map(renderEffectRow)}</div>
      </PanelSection>
        </>
      ) : null}

      {tab === "shape" ? (
        <PanelSection title="面部塑形" hint="局部形变，保持背景直线">
        {SHAPE_CONTROLS.map((control) => (
          <Slider
            key={control.key}
            label={control.label}
            hint={control.hint}
            value={params[control.key]}
            min={control.min}
            max={control.max}
            step={1}
            defaultValue={0}
            unit="%"
            disabled={(!layer && !activeFace) || (!params.geometry && !activeFace)}
            onChange={(value) => void handleShapeChange(control.key, value)}
            onCommit={() => void commitEdit(control.label)}
          />
        ))}
        <p className="inline-note">
          形变在颜色处理前作用于合成画面；拖动会自动创建或复用当前人脸的塑形图层，可随时归零或撤销。
        </p>
        <button
          type="button"
          className="panel-link panel-link--inline"
          onClick={() => setTool("retouch")}
        >
          推拉 / 收缩 / 膨胀液化在「修饰」工具中
        </button>
        </PanelSection>
      ) : null}

      {tab === "five" ? (
        <PanelSection
          title="精细修复"
          hint="只改实测到的眼睛、牙齿和皮肤区域"
        >
        <div className="switch-list">{FIVE_DETAIL_EFFECTS.map(renderEffectRow)}</div>
        <Collapsible
          title="五官细节"
          subtitle="眼白、虹膜与牙齿"
          defaultOpen={false}
          trailing={<span className="collapsible__badge">细分</span>}
        >
          {FIVE_DETAIL_CONTROLS.map((control) => (
            <Slider
              key={control.key}
              label={control.label}
              hint={control.hint}
              value={params[control.key]}
              min={0}
              max={100}
              step={1}
              defaultValue={0}
              unit="%"
              disabled={!layer && !activeFace}
              onChange={(value) => {
                void handleParamChange(control.key, value);
              }}
              onCommit={() => void commitEdit(control.label)}
            />
          ))}
        </Collapsible>
        <div className="fine-zones">
          <div className="fine-zones__head">
            <span>分区细纹</span>
            <span>可单独开关</span>
          </div>
          <div className="switch-list">
            {WRINKLE_ZONE_EFFECTS.map((effect) => (
              <div key={effect.key} className="fine-effect-row">
                <Toggle
                  label={effect.name}
                  description={effect.caption}
                  checked={params[effect.key] > 0}
                  disabled={
                    (!layer && !activeFace) ||
                    faceLayerBusy ||
                    (!params.geometry &&
                      !activeFace &&
                      params[effect.key] <= 0 &&
                      NEEDS_FACE_GEOMETRY.has(effect.key))
                  }
                  onChange={(checked) => {
                    void handleEffectToggle(effect.key, checked);
                  }}
                />
                {layer && params[effect.key] > 0 ? (
                  <Slider
                    label={`${effect.name}强度`}
                    value={params[effect.key]}
                    min={0}
                    max={100}
                    step={1}
                    defaultValue={effect.onValue}
                    unit="%"
                    onChange={(value) => {
                      editPortrait(layer.id, { ...params, [effect.key]: value });
                    }}
                    onCommit={() => void commitEdit(`微调${effect.name}`)}
                  />
                ) : null}
              </div>
            ))}
          </div>
          <p className="inline-note">
            “全部细纹”是总开关；关闭后可只用下面的分区开关单独处理。
          </p>
        </div>
        <p className="inline-note">
          <Sparkles size={12} aria-hidden="true" />
          {activeFaceRegions
            ? "已使用实测五官区域；没有测量到的部位会按人脸比例定位，可点上方“识别五官区域”提高精度。"
            : "打开任一精细修复会自动测量五官；未测量时按人脸比例定位。"}
        </p>
                </PanelSection>
      ) : null}

      {tab === "makeup" ? (
        <PanelSection title="妆容" hint="眼影、眉色、唇色与鼻影">
          {MAKEUP_CONTROLS.map((control) => (
            <Slider
              key={control.key}
              label={control.label}
              hint={control.hint}
              value={params[control.key]}
              min={0}
              max={100}
              step={1}
              defaultValue={0}
              unit="%"
              disabled={!layer && !activeFace}
              onChange={(value) => {
                void handleParamChange(control.key, value);
              }}
              onCommit={() => void commitEdit(control.label)}
            />
          ))}
          {layer && params.lipTint > 0 ? (
            <Slider
              label="唇色冷暖"
              hint="负值偏冷/莓果，正值偏暖/珊瑚"
              value={params.lipWarmth}
              min={-100}
              max={100}
              step={1}
              defaultValue={0}
              disabled={!layer}
              onChange={(value) => {
                if (layer) {
                  editPortrait(layer.id, { ...params, lipWarmth: value });
                }
              }}
              onCommit={() => void commitEdit("唇色冷暖")}
            />
          ) : null}
          <p className="inline-note">
            妆容只作用于实测的五官区域，可单独归零或关闭；没有区域时会按人脸比例定位。
          </p>
        </PanelSection>
      ) : null}

      {tab === "body" ? (
        <PanelSection
          title="身体美颜"
          hint={bodyLayer ? "独立于脸部图层" : "开关会新建身体美颜层"}
        >
        <div className="switch-list">
          {BODY_PORTRAIT_EFFECTS.map((effect) => (
            <div key={effect.key} className="fine-effect-row">
              <Toggle
                label={effect.name}
                description={effect.caption}
                checked={bodyParams[effect.key] > 0}
                disabled={faceLayerBusy || mode !== "tauri"}
                onChange={(checked) => {
                  void handleBodyEffectToggle(effect.key, checked);
                }}
              />
              {bodyLayer && bodyParams[effect.key] > 0 ? (
                <Slider
                  label={`${effect.name}强度`}
                  value={bodyParams[effect.key]}
                  min={0}
                  max={100}
                  step={1}
                  defaultValue={effect.onValue}
                  unit="%"
                  onChange={(value) => {
                    editPortrait(bodyLayer.id, { ...bodyParams, [effect.key]: value });
                  }}
                  onCommit={() => void commitEdit(`微调${effect.name}`)}
                />
              ) : null}
            </div>
          ))}
        </div>
        <div className="body-shape-block">
          <Slider
            label="身体收窄"
            hint="只在该图层的修饰范围内水平收窄；脸部自动保护。先涂抹腰部或身体范围再调整。"
            value={bodyParams.bodySlim}
            min={0}
            max={100}
            step={1}
            defaultValue={0}
            unit="%"
            disabled={mode !== "tauri" || faceLayerBusy}
            onChange={(value) => void handleBodyShapeChange("bodySlim", value)}
            onCommit={() => void commitEdit("身体收窄")}
          />
          <Slider
            label="腿部拉长"
            hint="只拉伸蒙版下半部分，适合拉长腿部；上方与蒙版外保持不变。"
            value={bodyParams.bodyLengthen}
            min={0}
            max={100}
            step={1}
            defaultValue={0}
            unit="%"
            disabled={mode !== "tauri" || faceLayerBusy}
            onChange={(value) => void handleBodyShapeChange("bodyLengthen", value)}
            onCommit={() => void commitEdit("腿部拉长")}
          />
          {mode === "tauri" &&
          (bodyParams.bodySlim > 0 || bodyParams.bodyLengthen > 0) &&
          !maskHasContent(bodyLayer?.mask) ? (
            <p className="inline-note inline-note--amber">
              还没有可用的塑形范围：请在该图层的「修饰范围」里涂抹身体或腿部，塑形才会生效。
            </p>
          ) : null}
        </div>
        <p className="inline-note">
          {bodyLayer
            ? "身体效果使用身体肤色范围，并避开当前选中的人脸；可在图层面板继续用画笔限定。"
            : "第一次开启会新建“身体美颜”图层，不要求人脸检测，也不会改动原图。"}
        </p>
        {mode !== "tauri" ? (
          <p className="inline-note">浏览器演示模式没有完整的身体渲染管线，身体美颜仅在桌面版可用。</p>
        ) : null}
        </PanelSection>
      ) : null}

      {tab === "advanced" ? (
        <>
      <section className="panel-section">
        <Collapsible
          title="更多调整"
          subtitle="想更细一点时再展开"
          defaultOpen={false}
          trailing={<span className="collapsible__badge">高级</span>}
        >
          <div className={cn(!layer && "controls-disabled")}>
            {ADVANCED_CONTROLS.map((control) => (
              <Slider
                key={control.key}
                label={control.label}
                hint={control.hint}
                value={params[control.key]}
                min={0}
                max={100}
                step={1}
                defaultValue={DEFAULT_PORTRAIT[control.key]}
                unit="%"
                disabled={!layer}
                onChange={(value) => {
                  if (layer) editPortrait(layer.id, { ...params, [control.key]: value });
                }}
                onCommit={() => void commitEdit("微调人像效果")}
              />
            ))}
          </div>
          <button
            type="button"
            className="panel-link"
            disabled={!layer}
            onClick={() => {
              if (layer) void resetPortrait(layer.id);
            }}
          >
            <RotateCcw size={11} aria-hidden="true" />
            恢复默认人像设置
          </button>
        </Collapsible>
      </section>

      <PanelSection title="五官区域" hint="按人脸加入蒙版">
        <div className="face-detect">
          <p>
            区域识别在每个人脸框内测量眼睛、眉毛、嘴唇与皮肤的位置；结果可按人加入当前图层的修饰范围。
          </p>
          <div className="region-actions">
            <Button
              variant="secondary"
              size="sm"
              icon={<ScanFace size={14} />}
              loading={regionBusy}
              disabled={!api || mode !== "tauri" || faces.length === 0}
              onClick={() => void analyzeFaceRegions()}
            >
              识别五官区域
            </Button>
            {faceRegions.length > 0 ? (
              <button
                type="button"
                className="panel-link panel-link--inline"
                aria-pressed={regionOverlayActive}
                onClick={() => setRegionOverlayActive(!regionOverlayActive)}
              >
                {regionOverlayActive ? "隐藏区域" : "显示区域"}
              </button>
            ) : null}
            <Button
              variant={geometryEditActive ? "secondary" : "ghost"}
              size="sm"
              icon={<Move size={14} />}
              aria-pressed={geometryEditActive}
              disabled={!api || mode !== "tauri" || faces.length === 0 || regionBusy}
              title={
                geometryEditActive
                  ? "完成并保存本次五官位置调整"
                  : "在画布上拖动椭圆，校正检测出的脸型与五官位置"
              }
              onClick={() => void setGeometryEditActive(!geometryEditActive)}
            >
              {geometryEditActive ? "完成调整" : "调整五官位置"}
            </Button>
          </div>
          {geometryEditActive ? (
            <p className="inline-note">
              拖动椭圆移动位置；拖动右侧或下方圆点改变范围。修改会写入当前人脸图层，可撤销。
            </p>
          ) : null}
        </div>
        {mode !== "tauri" ? (
          <p className="inline-note">
            浏览器演示模式没有五官定位，区域蒙版仅在桌面版可用。
          </p>
        ) : null}
        {activeFace && activeFaceRegions ? (
          <p className="inline-note">
            当前显示：人脸 {selectedFaceIndex + 1} 的五官区域。
          </p>
        ) : null}
        {activeFaceRegions && activeFaceRegions.regions.length > 0 ? (
          <div className="region-chips" role="group" aria-label="可加入蒙版的五官区域">
            {activeFaceRegions.regions
              .filter((region) => MASKABLE_REGIONS.includes(region.kind as never))
              .map((region) => {
                const used = maskRegionKinds.includes(region.kind);
                return (
                  <button
                    key={region.kind}
                    type="button"
                    className={cn("region-chip", used && "is-used")}
                    title={
                      used
                        ? `${region.label}已在当前图层的修饰范围内，点击移除`
                        : `把${region.label}加入当前图层的修饰范围`
                    }
                    onClick={() => {
                      if (used && regionLayerId) {
                        const index = maskRegionIndexes[region.kind];
                        if (index >= 0) void removeMaskRegion(regionLayerId, index);
                        return;
                      }
                      void handleRegionChip(region);
                    }}
                  >
                    {region.label}
                    <span className="region-chip__score">
                      {Math.round(region.confidence * 100)}%
                    </span>
                  </button>
                );
              })}
          </div>
        ) : null}
      </PanelSection>

      <section className="panel-section">
        <div className="model-card">
          <div className="model-card__head">
            <span className="model-card__icon">
              <LockKeyhole size={15} />
            </span>
            <div>
              <p className="model-card__title">本地五官几何</p>
              <span className="badge badge--green">已启用</span>
            </div>
          </div>
          <p className="model-card__desc">
            眼睛、牙齿、黑眼圈、皱纹和斑点修复只作用在实测的五官区域内；几何随图层保存，预览与导出不重复跑检测。
          </p>
          <p className="model-card__notice">
            磨皮、肤色均匀与提亮仍按肤色范围处理；没有测量到五官时会按人脸比例定位，不会跳过整张脸。
          </p>
        </div>
      </section>
        </>
      ) : null}
    </div>
  );
}
