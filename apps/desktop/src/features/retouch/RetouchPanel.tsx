import { Eraser, Expand, Grid3X3, Move, Shield, Shrink, Stamp } from "lucide-react";
import { cn } from "../../lib/cn";
import { useEditorStore, type RetouchToolMode } from "../../stores/editorStore";
import { PanelHeader } from "../../components/PanelHeader";
import { Slider } from "../../components/controls/Slider";

const REPAIR_MODES: Array<{
  value: RetouchToolMode;
  label: string;
  hint: string;
  icon: typeof Eraser;
}> = [
  {
    value: "heal",
    label: "修复画笔",
    hint: "从对面镜像取样并对齐周围颜色，适合去掉瑕疵",
    icon: Eraser,
  },
  {
    value: "clone",
    label: "克隆图章",
    hint: "从指定取样点复制像素，适合复制纹理；Alt 点击取样",
    icon: Stamp,
  },
];

const LIQUIFY_MODES: Array<{
  value: RetouchToolMode;
  label: string;
  hint: string;
  icon: typeof Eraser;
}> = [
  {
    value: "push",
    label: "推拉",
    hint: "按住并拖动，沿拖动方向推移像素",
    icon: Move,
  },
  {
    value: "pinch",
    label: "收缩",
    hint: "向笔刷中心收拢，适合收窄鼻翼或脸缘",
    icon: Shrink,
  },
  {
    value: "bulge",
    label: "膨胀",
    hint: "从笔刷中心向外放大，适合放大眼睛或局部",
    icon: Expand,
  },
  {
    value: "freeze",
    label: "冻结保护",
    hint: "涂过的区域在液化时保持不动，适合保护背景直线、发丝与身体轮廓",
    icon: Shield,
  },
  {
    value: "grid",
    label: "网格保护",
    hint: "点按格子冻结整块区域；网格覆盖全画面，裁剪后仍对齐",
    icon: Grid3X3,
  },
];

const LIQUIFY_KEYS: RetouchToolMode[] = ["push", "pinch", "bulge"];
const FREEZE_MODE: RetouchToolMode = "freeze";

export function RetouchPanel() {
  const mode = useEditorStore((state) => state.mode);
  const retouchMode = useEditorStore((state) => state.retouchMode);
  const radius = useEditorStore((state) => state.retouchRadius);
  const strength = useEditorStore((state) => state.retouchStrength);
  const source = useEditorStore((state) => state.retouchSource);
  const setMode = useEditorStore((state) => state.setRetouchMode);
  const setRadius = useEditorStore((state) => state.setRetouchRadius);
  const setStrength = useEditorStore((state) => state.setRetouchStrength);
  const setSource = useEditorStore((state) => state.setRetouchSource);
  const clearFreezeStrokes = useEditorStore((state) => state.clearFreezeStrokes);
  const layer = useEditorStore(
    (state) => state.doc?.layers.find((item) => item.kind === "retouch") ?? null,
  );
  const spots = layer?.retouch?.spots.length ?? 0;
  const warpStrokes = layer?.retouch?.warp.length ?? 0;
  const freezeStrokes = layer?.retouch?.freeze.length ?? 0;
  const freezeGridCount = layer?.retouch?.freezeGrid.filter(Boolean).length ?? 0;
  const liquifyActive = LIQUIFY_KEYS.includes(retouchMode);

  return (
    <div className="panel">
      <PanelHeader
        icon={<Stamp size={18} />}
        title="局部修复"
        description="在画布上点按或拖动，每一笔都可撤销"
      />

      <section className="panel-section">
        <div className="section-heading">
          <span>修复</span>
          <span className="section-note">
            {spots > 0 || warpStrokes > 0 || freezeStrokes > 0 || freezeGridCount > 0
              ? `${spots} 修复 · ${warpStrokes} 液化 · ${freezeStrokes} 笔保护 · ${freezeGridCount} 格`
              : "还没有修饰笔触"}
          </span>
        </div>
        <div className="preset-grid">
          {REPAIR_MODES.map((item) => {
            const Icon = item.icon;
            const active = retouchMode === item.value;
            return (
              <button
                key={item.value}
                type="button"
                className={cn("preset-card", active && "is-active")}
                aria-pressed={active}
                onClick={() => setMode(item.value)}
                title={item.hint}
              >
                <span className="preset-card__icon">
                  <Icon size={15} />
                </span>
                <span className="preset-card__name">{item.label}</span>
                <span className="preset-card__desc">{item.hint}</span>
              </button>
            );
          })}
        </div>
      </section>

      <section className="panel-section">
        <div className="section-heading">
          <span>液化</span>
          <span className="section-note">局部形变，不改原图</span>
        </div>
        <div className="style-chips" role="group" aria-label="液化方式">
          {LIQUIFY_MODES.map((item) => {
            const Icon = item.icon;
            const active = retouchMode === item.value;
            return (
              <button
                key={item.value}
                type="button"
                className={cn("style-chip", active && "is-active")}
                aria-pressed={active}
                title={item.hint}
                onClick={() => setMode(item.value)}
              >
                <span className="style-chip__icon">
                  <Icon size={13} />
                </span>
                {item.label}
              </button>
            );
          })}
        </div>
      </section>

      <section className="panel-section">
        <Slider
          label="笔刷大小"
          value={Math.round(radius * 1000)}
          min={4}
          max={200}
          step={2}
          defaultValue={30}
          unit="‰"
          compact
          showReset={false}
          onChange={(value) => setRadius(value / 1000)}
        />
        {liquifyActive && retouchMode !== FREEZE_MODE ? (
          <Slider
            label="液化强度"
            hint="越高位移越明显；可随时撤销"
            value={Math.round(strength * 100)}
            min={5}
            max={100}
            step={1}
            defaultValue={50}
            unit="%"
            onChange={(value) => setStrength(value / 100)}
          />
        ) : null}
        <p className="inline-note">
          {retouchMode === "clone"
            ? source
              ? `取样点已设（${Math.round(source.x * 100)}% × ${Math.round(source.y * 100)}%）。按住 Alt 点击可重新取样。`
              : "按住 Alt 在画布上点击一处作为取样点，然后直接点击要覆盖的位置。"
            : retouchMode === "heal"
              ? "修复画笔自动从对面镜像取样；边缘会与周围肤色对齐。"
              : retouchMode === "push"
                ? "按住并拖动，像素会沿拖动方向推移；适合调整脸缘、发丝或衣纹。"
                : retouchMode === "pinch"
                  ? "点击或拖动把像素向笔刷中心收拢；适合收窄鼻翼、脸缘或局部轮廓。"
                  : retouchMode === "freeze"
                    ? "涂过的区域会完全冻结：之后的推拉/收缩/膨胀都不会移动它，适合保护背景直线与发丝。"
                    : retouchMode === "grid"
                      ? "点按格子切换保护；冻结的格子不会被液化移动，适合快速保护门框、桌沿或身体边缘。"
                      : "点击或拖动把像素从笔刷中心向外放大；适合眼睛、嘴唇等局部。"}
        </p>
        {freezeStrokes > 0 || freezeGridCount > 0 ? (
          <button
            type="button"
            className="panel-link"
            onClick={() => void clearFreezeStrokes()}
          >
            清除冻结保护（{freezeStrokes} 笔
            {freezeGridCount > 0 ? ` · ${freezeGridCount} 格` : ""}）
          </button>
        ) : null}
        {retouchMode === "clone" && source ? (
          <button type="button" className="panel-link" onClick={() => setSource(null)}>
            清除取样点
          </button>
        ) : null}
        <p className="inline-note">快捷键：[ / ] 调整笔刷大小，Esc 退出画笔。</p>
        {mode !== "tauri" ? (
          <p className="inline-note">浏览器演示模式不执行局部修复或液化，请在桌面版使用。</p>
        ) : null}
      </section>
    </div>
  );
}
