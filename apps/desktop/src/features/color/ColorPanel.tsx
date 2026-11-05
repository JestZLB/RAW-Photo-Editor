import { useEffect, useRef, useState, type ReactNode } from "react";
import {
  ClipboardPaste,
  Copy,
  Film,
  Pipette,
  RotateCcw,
  SlidersHorizontal,
  Sparkles,
  Wand2,
} from "lucide-react";
import type { ColorParams } from "../../api/types";
import { cn } from "../../lib/cn";
import { CUBE_ACCEPT_ATTR } from "../../lib/file";
import { formatSigned } from "../../lib/format";
import { selectedOrKind } from "../../lib/layers";
import {
  COLOR_PRESETS,
  DEFAULT_COLOR,
  isDefaultColor,
  type ColorPresetId,
} from "../../lib/params";
import { latestAiDenoiseLayer, useEditorStore } from "../../stores/editorStore";
import { useSettingsStore } from "../../stores/settingsStore";
import { PanelHeader } from "../../components/PanelHeader";
import { IconButton } from "../../components/controls/Button";
import { Collapsible } from "../../components/controls/Collapsible";
import { HistogramView } from "../../components/controls/HistogramView";
import { PanelSection } from "../../components/controls/PanelSection";
import { Slider } from "../../components/controls/Slider";
import { Spinner } from "../../components/controls/Spinner";
import { Toggle } from "../../components/controls/Toggle";
import { ToneCurveEditor } from "../../components/controls/ToneCurveEditor";

type NumericColorKey = {
  [K in keyof ColorParams]: ColorParams[K] extends number ? K : never;
}[keyof ColorParams];

interface ColorControl {
  key: NumericColorKey;
  label: string;
  min: number;
  max: number;
  step: number;
  unit: string;
  format?: (value: number) => string;
}

const SIGNED = (value: number): string => formatSigned(value, 0);

const LIGHT_CONTROLS: ColorControl[] = [
  {
    key: "exposure",
    label: "曝光",
    min: -3,
    max: 3,
    step: 0.01,
    unit: "EV",
    format: (value) => formatSigned(value, 2),
  },
  { key: "contrast", label: "对比度", min: -100, max: 100, step: 1, unit: "", format: SIGNED },
  { key: "highlights", label: "高光", min: -100, max: 100, step: 1, unit: "", format: SIGNED },
  { key: "shadows", label: "阴影", min: -100, max: 100, step: 1, unit: "", format: SIGNED },
  { key: "whites", label: "白色色阶", min: -100, max: 100, step: 1, unit: "", format: SIGNED },
  { key: "blacks", label: "黑色色阶", min: -100, max: 100, step: 1, unit: "", format: SIGNED },
];

const WHITE_BALANCE_CONTROLS: ColorControl[] = [
  { key: "temperature", label: "色温", min: -100, max: 100, step: 1, unit: "", format: SIGNED },
  { key: "tint", label: "色调", min: -100, max: 100, step: 1, unit: "", format: SIGNED },
];

const COLOR_CONTROLS: ColorControl[] = [
  { key: "saturation", label: "饱和度", min: -100, max: 100, step: 1, unit: "", format: SIGNED },
  { key: "vibrance", label: "鲜艳度", min: -100, max: 100, step: 1, unit: "", format: SIGNED },
];

const HSL_CONTROLS: ColorControl[] = [
  {
    key: "hue",
    label: "色相",
    min: -180,
    max: 180,
    step: 1,
    unit: "°",
    format: (value) => `${value > 0 ? "+" : ""}${Math.round(value)}`,
  },
  { key: "hslSaturation", label: "饱和度", min: -100, max: 100, step: 1, unit: "", format: SIGNED },
  { key: "hslLightness", label: "明度", min: -100, max: 100, step: 1, unit: "", format: SIGNED },
];

const DETAIL_CONTROLS: ColorControl[] = [
  { key: "clarity", label: "通透感", min: -100, max: 100, step: 1, unit: "", format: SIGNED },
  { key: "sharpness", label: "锐度", min: 0, max: 100, step: 1, unit: "", format: (value) => `${Math.round(value)}` },
];

const EFFECT_CONTROLS: ColorControl[] = [
  { key: "vignette", label: "暗角", min: -100, max: 100, step: 1, unit: "", format: SIGNED },
  { key: "grain", label: "颗粒", min: 0, max: 100, step: 1, unit: "", format: (value) => `${Math.round(value)}` },
  { key: "dehaze", label: "去雾", min: 0, max: 100, step: 1, unit: "", format: (value) => `${Math.round(value)}` },
  { key: "chromatic", label: "色差校正", min: 0, max: 100, step: 1, unit: "", format: (value) => `${Math.round(value)}` },
];

function matchesPreset(params: ColorParams, presetId: ColorPresetId): boolean {
  const preset = COLOR_PRESETS.find((item) => item.id === presetId);
  if (!preset) return false;
  return (Object.keys(preset.settings) as NumericColorKey[]).every(
    (key) => Math.abs(params[key] - preset.settings[key]) < 0.01,
  );
}

export function ColorPanel() {
  const doc = useEditorStore((state) => state.doc);
  const preview = useEditorStore((state) => state.preview);
  const selectedLayerId = useEditorStore((state) => state.selectedLayerId);
  const applyAutoTone = useEditorStore((state) => state.applyAutoTone);
  const applyColorPreset = useEditorStore((state) => state.applyColorPreset);
  const commitEdit = useEditorStore((state) => state.commitEdit);
  const resetColor = useEditorStore((state) => state.resetColor);
  const copiedAdjustments = useEditorStore((state) => state.copiedAdjustments);
  const copyLayerAdjustments = useEditorStore((state) => state.copyLayerAdjustments);
  const pasteLayerAdjustments = useEditorStore((state) => state.pasteLayerAdjustments);
  const whiteBalancePickActive = useEditorStore((state) => state.whiteBalancePickActive);
  const whiteBalancePicking = useEditorStore((state) => state.whiteBalancePicking);
  const lastWhiteBalanceSample = useEditorStore((state) => state.lastWhiteBalanceSample);
  const setWhiteBalancePickActive = useEditorStore((state) => state.setWhiteBalancePickActive);
  const lutBusy = useEditorStore((state) => state.lutBusy);
  const lutAssets = useEditorStore((state) => state.lutAssets);
  const importLut = useEditorStore((state) => state.importLut);
  const removeLut = useEditorStore((state) => state.removeLut);
  const aiDenoiseBusy = useEditorStore((state) => state.aiDenoiseBusy);
  const photoHqBusy = useEditorStore((state) => state.photoHqBusy);
  const aiCapability = useEditorStore((state) => state.aiCapability);
  const aiCapabilityBusy = useEditorStore((state) => state.aiCapabilityBusy);
  const setAiDenoiseEnabled = useEditorStore((state) => state.setAiDenoiseEnabled);
  const setLayerOpacity = useEditorStore((state) => state.setLayerOpacity);
  const setLayerVisibility = useEditorStore((state) => state.setLayerVisibility);
  const aiDenoiseStrength = useSettingsStore((state) => state.aiDenoiseStrength);
  const photoHqStrength = useSettingsStore((state) => state.photoHqStrength);
  const aiDenoiseEngine = useSettingsStore((state) => state.aiDenoiseEngine);
  const [applyingPreset, setApplyingPreset] = useState<ColorPresetId | null>(null);
  const [autoToneBusy, setAutoToneBusy] = useState(false);
  const [aiFallbackNote, setAiFallbackNote] = useState<string | null>(null);
  const [photoHqSeconds, setPhotoHqSeconds] = useState(0);
  const cancelPhotoHqDenoise = useEditorStore((state) => state.cancelPhotoHqDenoise);
  const lutInputRef = useRef<HTMLInputElement>(null);
  const isTauri = useEditorStore((state) => state.mode) === "tauri";

  const layer = selectedOrKind(doc, selectedLayerId, "color");
  const params: ColorParams = { ...DEFAULT_COLOR, ...(layer?.color ?? {}) };
  const lutInfo = params.lutAssetId ? (lutAssets[params.lutAssetId] ?? null) : null;
  const lutLabel = lutInfo?.name ?? "已导入的 LUT";
  const aiLayer = latestAiDenoiseLayer(doc);
  const aiEnabled = Boolean(aiLayer?.visible);
  const defaultDenoiseStrength =
    aiDenoiseEngine === "photoHq" ? photoHqStrength : aiDenoiseStrength;
  const regularStrength = Math.round(params.noiseLuma);
  const aiBusy = aiDenoiseBusy || photoHqBusy;
  const denoiseOn = aiBusy || aiEnabled || params.noiseLuma > 0;
  const denoiseStrength = aiEnabled
    ? Math.round((aiLayer?.opacity ?? 1) * 100)
    : regularStrength > 0
      ? regularStrength
      : defaultDenoiseStrength;
  const aiKnownUnavailable =
    aiCapability !== null &&
    !aiCapability.photoHq.available &&
    !aiCapability.onnx.available;

  useEffect(() => {
    setAiFallbackNote(null);
  }, [doc?.id]);

  useEffect(() => {
    if (!photoHqBusy) {
      setPhotoHqSeconds(0);
      return;
    }
    const started = Date.now();
    setPhotoHqSeconds(0);
    const timer = window.setInterval(() => {
      setPhotoHqSeconds(Math.floor((Date.now() - started) / 1000));
    }, 500);
    return () => window.clearInterval(timer);
  }, [photoHqBusy]);

  const updateParams = (patch: Partial<ColorParams>): void => {
    const state = useEditorStore.getState();
    const currentLayer = selectedOrKind(state.doc, state.selectedLayerId, "color");
    if (currentLayer) {
      state.editColor(currentLayer.id, { ...(currentLayer.color ?? DEFAULT_COLOR), ...patch });
      return;
    }
    void state.ensureColorLayer().then((created) => {
      if (!created) return;
      const latest = useEditorStore.getState().doc?.layers.find((item) => item.id === created.id);
      useEditorStore
        .getState()
        .editColor(created.id, { ...(latest?.color ?? DEFAULT_COLOR), ...patch });
    });
  };

  const handleChange = (key: NumericColorKey, value: number): void => {
    updateParams({ [key]: value } as Partial<ColorParams>);
  };

  const handleDenoiseToggle = async (on: boolean): Promise<void> => {
    setAiFallbackNote(null);
    if (on) {
      updateParams({ noiseLuma: denoiseStrength > 0 ? denoiseStrength : 40 });
      await commitEdit("开启降噪");
      return;
    }
    if (aiLayer?.visible) await setLayerVisibility(aiLayer.id, false);
    updateParams({ noiseLuma: 0, noiseChroma: 0 });
    await commitEdit("关闭降噪");
  };

  const handleAiToggle = async (on: boolean): Promise<void> => {
    setAiFallbackNote(null);
    if (!on) {
      await setAiDenoiseEnabled(false, denoiseStrength);
      return;
    }
    const result = await setAiDenoiseEnabled(
      true,
      denoiseStrength > 0 ? denoiseStrength : defaultDenoiseStrength,
    );
    if (!result.ok) {
      if (result.projectChanged) return;
      setAiFallbackNote(
        result.engine
          ? (result.reason ?? "AI 降噪运行失败，已使用普通降噪")
          : "没有可用的 AI 降噪引擎，已保持普通降噪",
      );
      updateParams({
        noiseLuma: denoiseStrength > 0 ? denoiseStrength : defaultDenoiseStrength,
        noiseChroma: 0,
      });
      await commitEdit("使用普通降噪");
    }
  };

  const handlePreset = async (presetId: ColorPresetId): Promise<void> => {
    setApplyingPreset(presetId);
    try {
      await applyColorPreset(presetId);
    } finally {
      setApplyingPreset(null);
    }
  };

  const handleAutoTone = async (): Promise<void> => {
    if (autoToneBusy) return;
    setAutoToneBusy(true);
    try {
      await applyAutoTone();
    } finally {
      setAutoToneBusy(false);
    }
  };

  const renderControls = (controls: ColorControl[], sectionLabel: string): ReactNode => (
    <div>
      {controls.map((control) => (
        <Slider
          key={control.key}
          label={control.label}
          value={params[control.key]}
          min={control.min}
          max={control.max}
          step={control.step}
          defaultValue={DEFAULT_COLOR[control.key]}
          unit={control.unit}
          formatValue={control.format}
          onChange={(value) => handleChange(control.key, value)}
          onCommit={() => void commitEdit(`微调${sectionLabel}`)}
        />
      ))}
    </div>
  );

  return (
    <div className="panel">
      <PanelHeader
        icon={<SlidersHorizontal size={18} />}
        title="调色"
        description="让光线和肤色更自然，不用懂参数"
        action={
          layer || copiedAdjustments?.kind === "color" ? (
            <div className="panel-head-actions">
              {layer ? (
                <IconButton
                  label="复制调色参数"
                  icon={<Copy size={14} />}
                  size="sm"
                  onClick={() => copyLayerAdjustments(layer.id)}
                />
              ) : null}
              {copiedAdjustments?.kind === "color" ? (
                <IconButton
                  label="粘贴调色参数"
                  icon={<ClipboardPaste size={14} />}
                  size="sm"
                  onClick={() => void pasteLayerAdjustments()}
                />
              ) : null}
              {layer ? (
                <IconButton
                  label="复位调色"
                  icon={<RotateCcw size={14} />}
                  size="sm"
                  onClick={() => void resetColor(layer.id)}
                />
              ) : null}
            </div>
          ) : undefined
        }
      />

      <section className="panel-section">
        <div className="section-heading">
          <span>快速风格</span>
          <span className="section-note">一键起调</span>
        </div>
        <div className="color-presets" role="group" aria-label="调色风格">
          {COLOR_PRESETS.map((preset) => {
            const active =
              preset.id === "original" ? isDefaultColor(params) : matchesPreset(params, preset.id);
            return (
              <button
                key={preset.id}
                type="button"
                className={cn("color-chip", active && "is-active")}
                aria-pressed={active}
                title={preset.description}
                onClick={() => void handlePreset(preset.id)}
              >
                {applyingPreset === preset.id ? <Spinner size={11} /> : null}
                {preset.name}
              </button>
            );
          })}
        </div>
        {!layer ? (
          <p className="inline-note">
            <Sparkles size={12} aria-hidden="true" />
            第一次调整时会自动新建调色图层，原图始终保留。
          </p>
        ) : null}
      </section>

      <section className="panel-section">
        <div className="section-heading">
          <span>直方图</span>
          <span className="section-note">RGB · 亮度</span>
        </div>
        <HistogramView histogram={preview?.histogram ?? null} />
      </section>

      <section className="panel-section">
        <Collapsible
          title="白平衡"
          subtitle="先校正偏色"
          defaultOpen
          trailing={
            <button
              type="button"
              className="panel-link panel-link--inline"
              disabled={!layer}
              onClick={() => {
                updateParams({
                  temperature: DEFAULT_COLOR.temperature,
                  tint: DEFAULT_COLOR.tint,
                });
                void commitEdit("复位白平衡");
              }}
            >
              复位
            </button>
          }
        >
          <div className="white-balance-tool">
            <button
              type="button"
              className={cn(
                "white-balance-tool__btn",
                whiteBalancePickActive && "is-active",
                whiteBalancePicking && "is-busy",
              )}
              aria-pressed={whiteBalancePickActive}
              disabled={whiteBalancePicking || !doc}
              title="点击后，在画面中选一块中性灰或白色区域"
              onClick={() => setWhiteBalancePickActive(!whiteBalancePickActive)}
            >
              {whiteBalancePicking ? (
                <Spinner size={12} />
              ) : (
                <Pipette size={13} aria-hidden="true" />
              )}
              {whiteBalancePicking
                ? "正在读取采样点…"
                : whiteBalancePickActive
                  ? "请点击画面中的中性灰"
                  : "白平衡吸管"}
            </button>
            {lastWhiteBalanceSample ? (
              <span
                className="white-balance-swatch"
                style={{
                  background: `rgb(${lastWhiteBalanceSample.join(", ")})`,
                }}
                title={`上次采样：rgb(${lastWhiteBalanceSample.join(", ")})`}
                aria-label={`上次白平衡采样颜色 rgb(${lastWhiteBalanceSample.join(", ")})`}
              />
            ) : null}
          </div>
          <p className="inline-note">
            <Sparkles size={12} aria-hidden="true" />
            不用吸管也可以直接拖：色温向左更冷、向右更暖；色调向左偏绿、向右偏洋红。
          </p>
          {renderControls(WHITE_BALANCE_CONTROLS, "白平衡")}
        </Collapsible>
      </section>

      <section className="panel-section">
        <Collapsible
          title="光线"
          subtitle="曝光与明暗层次"
          defaultOpen
          trailing={
            <button
              type="button"
              className="panel-link panel-link--inline"
              disabled={!layer}
              onClick={() => {
                updateParams({
                  exposure: DEFAULT_COLOR.exposure,
                  contrast: DEFAULT_COLOR.contrast,
                  highlights: DEFAULT_COLOR.highlights,
                  shadows: DEFAULT_COLOR.shadows,
                  whites: DEFAULT_COLOR.whites,
                  blacks: DEFAULT_COLOR.blacks,
                });
                void commitEdit("复位光线");
              }}
            >
              复位
            </button>
          }
        >
          <div className="white-balance-tool">
            <button
              type="button"
              className={cn("white-balance-tool__btn", autoToneBusy && "is-busy")}
              disabled={autoToneBusy || whiteBalancePicking || !doc}
              title="根据原图直方图自动设置曝光、对比度与黑白场"
              onClick={() => void handleAutoTone()}
            >
              <Wand2 size={13} aria-hidden="true" />
              {autoToneBusy ? "正在分析…" : "自动校正"}
            </button>
          </div>
          {renderControls(LIGHT_CONTROLS, "光线")}
        </Collapsible>
      </section>

      <section className="panel-section">
        <Collapsible
          title="色彩"
          subtitle="饱和度与鲜艳度"
          defaultOpen
          trailing={
            <button
              type="button"
              className="panel-link panel-link--inline"
              disabled={!layer}
              onClick={() => {
                updateParams({
                  saturation: DEFAULT_COLOR.saturation,
                  vibrance: DEFAULT_COLOR.vibrance,
                });
                void commitEdit("复位色彩");
              }}
            >
              复位
            </button>
          }
        >
          {renderControls(COLOR_CONTROLS, "色彩")}
        </Collapsible>
      </section>

      <section className="panel-section">
        <Collapsible title="色相与明度" subtitle="整体 HSL 偏移" defaultOpen={false}>
          {renderControls(HSL_CONTROLS, "色相")}
        </Collapsible>
      </section>

      <PanelSection title="LUT 风格" hint="套用 .cube 胶片色">
        {params.lutAssetId ? (
          <>
            <div className="lut-head">
              <span className="lut-head__name" title={lutLabel}>
                {lutLabel}
              </span>
              {lutInfo ? (
                <span className="lut-head__tag">
                  {lutInfo.dimension} · {lutInfo.size}
                </span>
              ) : null}
              <button
                type="button"
                className="panel-link panel-link--inline"
                onClick={() => void removeLut()}
              >
                移除
              </button>
            </div>
            <Slider
              label="强度"
              value={Math.round((params.lutStrength ?? 1) * 100)}
              min={0}
              max={100}
              step={1}
              defaultValue={100}
              unit="%"
              onChange={(value) => updateParams({ lutStrength: value / 100 })}
              onCommit={() => void commitEdit("调整 LUT 强度")}
            />
          </>
        ) : (
          <div className="white-balance-tool">
            <button
              type="button"
              className={cn("white-balance-tool__btn", lutBusy && "is-busy")}
              disabled={!doc || lutBusy}
              title="导入 .cube 文件，给整张照片套上风格"
              onClick={() => {
                if (isTauri) {
                  void importLut(null);
                  return;
                }
                lutInputRef.current?.click();
              }}
            >
              {lutBusy ? <Spinner size={12} /> : <Film size={13} aria-hidden="true" />}
              {lutBusy ? "正在导入…" : "导入 .cube LUT"}
            </button>
            {isTauri ? null : (
              <input
                ref={lutInputRef}
                type="file"
                accept={CUBE_ACCEPT_ATTR}
                className="lut-file-input"
                aria-label="选择 .cube LUT 文件"
                tabIndex={-1}
                onChange={(event) => {
                  const file = event.target.files?.[0] ?? null;
                  event.target.value = "";
                  if (file) void importLut(file);
                }}
              />
            )}
          </div>
        )}
        <p className="inline-note">
          <Sparkles size={12} aria-hidden="true" />
          LUT 只在调色图层上记录，原图和参数都不受影响。
        </p>
      </PanelSection>

      <section className="panel-section">
        <Collapsible
          title="曲线与色阶"
          subtitle="专业微调，默认收起"
          defaultOpen={false}
          trailing={
            <button
              type="button"
              className="panel-link panel-link--inline"
              disabled={!layer}
              onClick={() => {
                updateParams({
                  curveX: DEFAULT_COLOR.curveX,
                  curveY: DEFAULT_COLOR.curveY,
                  levelsInBlack: DEFAULT_COLOR.levelsInBlack,
                  levelsInWhite: DEFAULT_COLOR.levelsInWhite,
                  levelsGamma: DEFAULT_COLOR.levelsGamma,
                  levelsOutBlack: DEFAULT_COLOR.levelsOutBlack,
                  levelsOutWhite: DEFAULT_COLOR.levelsOutWhite,
                });
                void commitEdit("复位曲线与色阶");
              }}
            >
              复位
            </button>
          }
        >
          <ToneCurveEditor
            points={params.curveX}
            values={params.curveY}
            histogram={preview?.histogram?.luma ?? null}
            onChange={(index, value) => {
              const next = [...params.curveY];
              next[index] = value;
              updateParams({ curveY: next });
            }}
            onCommit={() => void commitEdit("调整曲线")}
          />
          <Slider
            label="输入黑场"
            value={Math.round(params.levelsInBlack * 100)}
            min={0}
            max={90}
            step={1}
            defaultValue={0}
            unit="%"
            compact
            onChange={(value) => updateParams({ levelsInBlack: value / 100 })}
            onCommit={() => void commitEdit("调整色阶")}
          />
          <Slider
            label="输入白场"
            value={Math.round(params.levelsInWhite * 100)}
            min={10}
            max={100}
            step={1}
            defaultValue={100}
            unit="%"
            compact
            onChange={(value) => updateParams({ levelsInWhite: value / 100 })}
            onCommit={() => void commitEdit("调整色阶")}
          />
          <Slider
            label="中间调"
            value={Math.round(params.levelsGamma * 100)}
            min={10}
            max={1000}
            step={5}
            defaultValue={100}
            onChange={(value) => updateParams({ levelsGamma: value / 100 })}
            onCommit={() => void commitEdit("调整色阶")}
          />
          <Slider
            label="输出黑场"
            value={Math.round(params.levelsOutBlack * 100)}
            min={0}
            max={90}
            step={1}
            defaultValue={0}
            unit="%"
            compact
            onChange={(value) => updateParams({ levelsOutBlack: value / 100 })}
            onCommit={() => void commitEdit("调整色阶")}
          />
          <Slider
            label="输出白场"
            value={Math.round(params.levelsOutWhite * 100)}
            min={10}
            max={100}
            step={1}
            defaultValue={100}
            unit="%"
            compact
            onChange={(value) => updateParams({ levelsOutWhite: value / 100 })}
            onCommit={() => void commitEdit("调整色阶")}
          />
        </Collapsible>
      </section>

      <section className="panel-section">
        <Collapsible title="细节" subtitle="通透感与锐化" defaultOpen={false}>
          {renderControls(DETAIL_CONTROLS, "细节")}
        </Collapsible>
      </section>

      <section className="panel-section">
        <Collapsible
          title="效果"
          subtitle="降噪 · 暗角 · 颗粒 · 去雾 · 色差"
          defaultOpen={false}
          trailing={
            <button
              type="button"
              className="panel-link panel-link--inline"
              disabled={!layer && !aiLayer}
              onClick={() => {
                if (aiLayer?.visible) void setLayerVisibility(aiLayer.id, false);
                updateParams({
                  noiseLuma: DEFAULT_COLOR.noiseLuma,
                  noiseChroma: DEFAULT_COLOR.noiseChroma,
                  vignette: DEFAULT_COLOR.vignette,
                  grain: DEFAULT_COLOR.grain,
                  dehaze: DEFAULT_COLOR.dehaze,
                  chromatic: DEFAULT_COLOR.chromatic,
                });
                void commitEdit("复位效果");
              }}
            >
              复位
            </button>
          }
        >
          <div className="noise-block">
            <Toggle
              label="降噪"
              description="降低暗部噪点；关闭后整组参数不生效"
              checked={denoiseOn}
              disabled={!doc}
              onChange={(value) => void handleDenoiseToggle(value)}
            />
            {denoiseOn ? (
              <div className="noise-block__body">
                <Slider
                  label="降噪强度"
                  value={denoiseStrength}
                  min={0}
                  max={100}
                  step={1}
                  defaultValue={defaultDenoiseStrength}
                  unit="%"
                  onChange={(value) => {
                    if (aiEnabled && aiLayer) {
                      setLayerOpacity(aiLayer.id, value / 100);
                    } else {
                      updateParams({ noiseLuma: value });
                    }
                  }}
                  onCommit={() =>
                    void commitEdit(aiEnabled ? "调整 AI 降噪强度" : "调整降噪")
                  }
                />
                {aiEnabled ? null : (
                  <Slider
                    label="色彩降噪"
                    value={Math.round(params.noiseChroma)}
                    min={0}
                    max={100}
                    step={1}
                    defaultValue={DEFAULT_COLOR.noiseChroma}
                    unit="%"
                    compact
                    onChange={(value) => updateParams({ noiseChroma: value })}
                    onCommit={() => void commitEdit("调整色彩降噪")}
                  />
                )}
                <Toggle
                  label="AI 降噪"
                  checked={aiEnabled}
                  disabled={!isTauri || aiBusy || aiCapabilityBusy || aiKnownUnavailable}
                  description={
                    !isTauri
                      ? "本地 AI 推理仅在桌面版可用"
                      : aiCapabilityBusy
                        ? "正在检测本机 GPU 与模型…"
                        : aiBusy
                          ? "AI 降噪运行中，完成后会写入独立图层"
                          : aiKnownUnavailable
                          ? (aiCapability?.photoHq.reason ??
                            aiCapability?.onnx.reason ??
                            "本机没有可用的 AI 降噪引擎")
                          : aiEnabled
                            ? `已用 ${
                                aiLayer?.name.includes("Photo-HQ")
                                  ? "Photo-HQ-W4xEX"
                                  : "ONNX 模型"
                              }；强度由上方滑块控制`
                            : "自动检测 Photo-HQ 或 ONNX；不可用时保持普通降噪"
                  }
                  badge={
                    aiEnabled ? (
                      <span className="badge badge--green">
                        {aiLayer?.name.includes("Photo-HQ") ? "Photo-HQ" : "ONNX"}
                      </span>
                    ) : null
                  }
                  onChange={(value) => void handleAiToggle(value)}
                />
                {aiBusy ? (
                  <div className="inline-note ai-busy-row">
                    <span className="ai-busy-row__text">
                      <Spinner size={12} />
                      {photoHqBusy
                        ? `Photo-HQ 正在运行 ${photoHqSeconds}s，完成后自动写入图层；照片不会上传。`
                        : "AI 降噪运行中，完成后会自动把强度写入图层；照片不会上传。"}
                    </span>
                    {photoHqBusy ? (
                      <button
                        type="button"
                        className="panel-link panel-link--inline"
                        onClick={() => void cancelPhotoHqDenoise()}
                      >
                        取消
                      </button>
                    ) : null}
                  </div>
                ) : null}
                {aiFallbackNote ? (
                  <p className="inline-note inline-note--amber">{aiFallbackNote}</p>
                ) : null}
                {aiEnabled && !aiBusy ? (
                  <p className="inline-note">
                    <Sparkles size={12} aria-hidden="true" />
                    AI 结果保存在独立图层；关闭 AI 降噪会恢复普通降噪。
                  </p>
                ) : null}
              </div>
            ) : null}
          </div>
          <div className="effect-divider" />
          {renderControls(EFFECT_CONTROLS, "效果")}
        </Collapsible>
      </section>

      <section className="panel-section panel-section--compact">
        <button
          type="button"
          className="panel-link"
          disabled={!layer}
          onClick={() => {
            if (layer) void resetColor(layer.id);
          }}
        >
          <RotateCcw size={11} aria-hidden="true" />
          复位调色
        </button>
      </section>
    </div>
  );
}
