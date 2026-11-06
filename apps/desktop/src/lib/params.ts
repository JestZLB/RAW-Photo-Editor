import type { BlurParams, ColorParams, PortraitParams } from "../api/types";

export const DEFAULT_COLOR: ColorParams = {
  exposure: 0,
  contrast: 0,
  highlights: 0,
  shadows: 0,
  whites: 0,
  blacks: 0,
  temperature: 0,
  tint: 0,
  saturation: 0,
  vibrance: 0,
  clarity: 0,
  sharpness: 0,
  hue: 0,
  hslSaturation: 0,
  hslLightness: 0,
  curveX: [0, 0.25, 0.5, 0.75, 1],
  curveY: [0, 0.25, 0.5, 0.75, 1],
  levelsInBlack: 0,
  levelsInWhite: 1,
  levelsGamma: 1,
  levelsOutBlack: 0,
  levelsOutWhite: 1,
  noiseLuma: 0,
  noiseChroma: 0,
  vignette: 0,
  grain: 0,
  dehaze: 0,
  chromatic: 0,
  lutAssetId: null,
  lutStrength: 1,
};

/** Matches `BlurParams::default()` in the document crate. */
export const DEFAULT_BLUR: BlurParams = { radius: 0.012 };
/** Document-side clamp (`BlurParams::sanitized`). */
export const BLUR_RADIUS_MAX = 0.08;

export const DEFAULT_PORTRAIT: PortraitParams = {
  intensity: 78,
  smooth: 0,
  texture: 70,
  evenSkin: 0,
  brighten: 0,
  bodySmooth: 0,
  eyeEnhance: 0,
  teethWhiten: 0,
  blemish: 0,
  wrinkle: 0,
  darkCircle: 0,
  mole: 0,
  freckle: 0,
  birthmark: 0,
  tattoo: 0,
  whiten: 0,
  whitenWarmth: 0,
  rosy: 0,
  eyeBag: 0,
  wrinkleForehead: 0,
  wrinkleGlabella: 0,
  wrinkleCrowFeet: 0,
  wrinkleNasolabial: 0,
  wrinkleMouthCorner: 0,
  wrinkleUnderEye: 0,
  wrinkleNeck: 0,
  bodyMode: false,
  bodyBlemish: 0,
  bodyEven: 0,
  bodyBrighten: 0,
  bodyTexture: 0,
  bodyWhiten: 0,
  bodySlim: 0,
  bodyLengthen: 0,
  faceSlim: 0,
  eyeSize: 0,
  noseSlim: 0,
  smoothRadius: 0,
  whitenSaturation: 0,
  oilControl: 0,
  eyeWhite: 0,
  irisDetail: 0,
  teethBrighten: 0,
  lipTint: 0,
  lipWarmth: 0,
  noseContour: 0,
  eyeshadow: 0,
  browTint: 0,
  jawWidth: 0,
  chinLength: 0,
  eyeDistance: 0,
};

const RESTING_PORTRAIT: Omit<
  PortraitParams,
  "intensity" | "smooth" | "texture" | "evenSkin" | "brighten" | "bodySmooth"
> = {
  eyeEnhance: 0,
  teethWhiten: 0,
  blemish: 0,
  wrinkle: 0,
  darkCircle: 0,
  mole: 0,
  freckle: 0,
  birthmark: 0,
  tattoo: 0,
  whiten: 0,
  whitenWarmth: 0,
  rosy: 0,
  eyeBag: 0,
  wrinkleForehead: 0,
  wrinkleGlabella: 0,
  wrinkleCrowFeet: 0,
  wrinkleNasolabial: 0,
  wrinkleMouthCorner: 0,
  wrinkleUnderEye: 0,
  wrinkleNeck: 0,
  bodyMode: false,
  bodyBlemish: 0,
  bodyEven: 0,
  bodyBrighten: 0,
  bodyTexture: 0,
  bodyWhiten: 0,
  bodySlim: 0,
  bodyLengthen: 0,
  faceSlim: 0,
  eyeSize: 0,
  noseSlim: 0,
  smoothRadius: 0,
  whitenSaturation: 0,
  oilControl: 0,
  eyeWhite: 0,
  irisDetail: 0,
  teethBrighten: 0,
  lipTint: 0,
  lipWarmth: 0,
  noseContour: 0,
  eyeshadow: 0,
  browTint: 0,
  jawWidth: 0,
  chinLength: 0,
  eyeDistance: 0,
};

export type PortraitPresetId = "natural" | "clear" | "soft" | "creamy" | "oxygen" | "stage";

export interface PortraitPreset {
  id: PortraitPresetId;
  name: string;
  description: string;
  label: string;
  settings: PortraitParams;
}

function preset(
  smooth: number,
  texture: number,
  evenSkin: number,
  brighten: number,
): PortraitParams {
  return {
    ...DEFAULT_PORTRAIT,
    ...RESTING_PORTRAIT,
    intensity: 78,
    smooth,
    texture,
    evenSkin,
    brighten,
    bodySmooth: 0,
  };
}

function beautyPreset(
  base: PortraitParams,
  overrides: Partial<PortraitParams>,
): PortraitParams {
  return {
    ...DEFAULT_PORTRAIT,
    ...RESTING_PORTRAIT,
    ...base,
    ...overrides,
  };
}

export const PORTRAIT_PRESETS: PortraitPreset[] = [
  {
    id: "natural",
    name: "自然美颜",
    description: "轻磨皮，保留皮肤真实纹理",
    label: "应用自然美颜",
    settings: preset(22, 78, 12, 8),
  },
  {
    id: "clear",
    name: "清透提亮",
    description: "少量柔肤，整体更透亮",
    label: "应用清透提亮",
    settings: preset(14, 82, 16, 20),
  },
  {
    id: "soft",
    name: "轻度精修",
    description: "更柔和的肤质与均匀肤色",
    label: "应用轻度精修",
    settings: preset(38, 62, 24, 12),
  },
  {
    id: "creamy",
    name: "奶油肌",
    description: "柔焦、通透与自然气色",
    label: "应用奶油肌",
    settings: beautyPreset(preset(30, 55, 18, 10), {
      whiten: 55,
      whitenSaturation: 25,
      rosy: 18,
      oilControl: 20,
      eyeWhite: 15,
      teethBrighten: 10,
    }),
  },
  {
    id: "oxygen",
    name: "氧气感",
    description: "保留纹理的清透红润",
    label: "应用氧气感",
    settings: beautyPreset(preset(16, 82, 14, 12), {
      whiten: 35,
      rosy: 35,
      lipTint: 18,
      lipWarmth: 15,
      eyeWhite: 20,
    }),
  },
  {
    id: "stage",
    name: "舞台妆",
    description: "眼神、唇色与立体感的组合",
    label: "应用舞台妆",
    settings: beautyPreset(preset(24, 60, 20, 8), {
      whiten: 25,
      rosy: 45,
      lipTint: 45,
      lipWarmth: 20,
      eyeEnhance: 50,
      eyeWhite: 40,
      teethWhiten: 60,
      teethBrighten: 20,
      noseContour: 25,
    }),
  },
];

export const COLOR_PRESET_IDS = ["original", "soft", "clean", "film"] as const;
export type ColorPresetId = (typeof COLOR_PRESET_IDS)[number];

export interface ColorPreset {
  id: ColorPresetId;
  name: string;
  description: string;
  label: string;
  settings: ColorParams;
}

function color(overrides: Partial<ColorParams>): ColorParams {
  return { ...DEFAULT_COLOR, ...overrides };
}

export const COLOR_PRESETS: ColorPreset[] = [
  {
    id: "original",
    name: "原图",
    description: "回到未调色的干净状态",
    label: "恢复原始色调",
    settings: color({}),
  },
  {
    id: "soft",
    name: "柔和",
    description: "降低反差，肤色更温润",
    label: "应用柔和调色",
    settings: color({
      exposure: 0.12,
      contrast: -8,
      highlights: -12,
      shadows: 12,
      saturation: -4,
      vibrance: 8,
      temperature: 5,
      clarity: -6,
    }),
  },
  {
    id: "clean",
    name: "通透",
    description: "提亮暗部，画面更干净",
    label: "应用通透调色",
    settings: color({
      exposure: 0.08,
      contrast: 10,
      highlights: -18,
      shadows: 16,
      whites: 6,
      blacks: -4,
      saturation: 4,
      vibrance: 14,
      temperature: -3,
      clarity: 8,
      sharpness: 12,
    }),
  },
  {
    id: "film",
    name: "胶片",
    description: "暖调低饱和，轻微褪色感",
    label: "应用胶片调色",
    settings: color({
      exposure: -0.06,
      contrast: 14,
      highlights: -20,
      shadows: 20,
      blacks: -10,
      temperature: 14,
      tint: 4,
      saturation: -14,
      vibrance: 6,
      clarity: 4,
    }),
  },
];

export type BasePortraitEffectKey = "smooth" | "evenSkin" | "brighten";
export type FinePortraitEffectKey =
  | "eyeEnhance"
  | "teethWhiten"
  | "blemish"
  | "wrinkle"
  | "darkCircle"
  | "mole"
  | "freckle"
  | "birthmark"
  | "tattoo";
export type BeautyPortraitEffectKey = "whiten" | "rosy" | "eyeBag";
export type WrinkleZoneEffectKey =
  | "wrinkleForehead"
  | "wrinkleGlabella"
  | "wrinkleCrowFeet"
  | "wrinkleNasolabial"
  | "wrinkleMouthCorner"
  | "wrinkleUnderEye"
  | "wrinkleNeck";
export type BodyPortraitEffectKey =
  | "bodySmooth"
  | "bodyWhiten"
  | "bodyBlemish"
  | "bodyEven"
  | "bodyBrighten"
  | "bodyTexture";
export type PortraitEffectKey =
  | BasePortraitEffectKey
  | FinePortraitEffectKey
  | BeautyPortraitEffectKey
  | WrinkleZoneEffectKey
  | BodyPortraitEffectKey;

export interface PortraitEffectSwitch {
  key: PortraitEffectKey;
  name: string;
  caption: string;
  onValue: number;
  group: "base" | "fine" | "beauty" | "wrinkle" | "body";
}

export const PORTRAIT_EFFECTS: PortraitEffectSwitch[] = [
  {
    key: "smooth",
    name: "磨皮",
    caption: "柔化肤色，保留自然光影",
    onValue: 22,
    group: "base",
  },
  {
    key: "evenSkin",
    name: "肤色均匀",
    caption: "减轻脸颊泛红与色差",
    onValue: 12,
    group: "base",
  },
  {
    key: "brighten",
    name: "自然提亮",
    caption: "让肤色更明亮，不改变背景",
    onValue: 8,
    group: "base",
  },
  {
    key: "whiten",
    name: "自然美白",
    caption: "提亮肤色并减轻黄气，不改变身份特征",
    onValue: 45,
    group: "beauty",
  },
  {
    key: "rosy",
    name: "红润气色",
    caption: "给脸颊加一点自然血色，不覆盖五官",
    onValue: 35,
    group: "beauty",
  },
  {
    key: "eyeBag",
    name: "眼袋阴影",
    caption: "减轻眼下袋状阴影与暗沉，不做结构重塑",
    onValue: 45,
    group: "beauty",
  },
  {
    key: "eyeEnhance",
    name: "眼睛提亮",
    caption: "提亮眼神，保留瞳孔的深色",
    onValue: 45,
    group: "fine",
  },
  {
    key: "teethWhiten",
    name: "牙齿美白",
    caption: "减轻牙齿的黄色，不漂白嘴唇",
    onValue: 55,
    group: "fine",
  },
  {
    key: "darkCircle",
    name: "黑眼圈",
    caption: "提亮眼下的阴影与暗沉",
    onValue: 45,
    group: "fine",
  },
  {
    key: "wrinkle",
    name: "全部细纹",
    caption: "同时处理额头、眉间、眼角与法令纹",
    onValue: 40,
    group: "fine",
  },
  {
    key: "blemish",
    name: "瑕疵修复",
    caption: "修复痘印、小斑点和局部泛红",
    onValue: 50,
    group: "fine",
  },
  {
    key: "mole",
    name: "痣",
    caption: "淡化深色痣，保留皮肤质感",
    onValue: 45,
    group: "fine",
  },
  {
    key: "freckle",
    name: "雀斑",
    caption: "减轻成片雀斑与色素点",
    onValue: 40,
    group: "fine",
  },
  {
    key: "birthmark",
    name: "胎记",
    caption: "柔化较大的深色或红色胎记",
    onValue: 35,
    group: "fine",
  },
  {
    key: "tattoo",
    name: "纹身",
    caption: "淡化皮肤上的深色纹身图案",
    onValue: 30,
    group: "fine",
  },
  {
    key: "wrinkleForehead",
    name: "额头纹",
    caption: "淡化横向抬头纹",
    onValue: 35,
    group: "wrinkle",
  },
  {
    key: "wrinkleGlabella",
    name: "眉间纹",
    caption: "淡化两眉之间的竖向纹路",
    onValue: 35,
    group: "wrinkle",
  },
  {
    key: "wrinkleCrowFeet",
    name: "鱼尾纹",
    caption: "淡化眼角外侧的放射状细纹",
    onValue: 35,
    group: "wrinkle",
  },
  {
    key: "wrinkleNasolabial",
    name: "法令纹",
    caption: "淡化鼻翼到嘴角的阴影",
    onValue: 42,
    group: "wrinkle",
  },
  {
    key: "wrinkleMouthCorner",
    name: "嘴角纹",
    caption: "淡化嘴角下垂处的细纹",
    onValue: 35,
    group: "wrinkle",
  },
  {
    key: "wrinkleUnderEye",
    name: "眼下细纹",
    caption: "淡化下眼睑下方的小干纹",
    onValue: 32,
    group: "wrinkle",
  },
  {
    key: "wrinkleNeck",
    name: "颈纹",
    caption: "淡化颈部横向纹路，需身体美颜层",
    onValue: 35,
    group: "wrinkle",
  },
  {
    key: "bodySmooth",
    name: "身体磨皮",
    caption: "柔化颈部、肩臂与腿部皮肤",
    onValue: 18,
    group: "body",
  },
  {
    key: "bodyWhiten",
    name: "身体美白",
    caption: "提亮身体肤色并减轻色差",
    onValue: 35,
    group: "body",
  },
  {
    key: "bodyBlemish",
    name: "身体瑕疵",
    caption: "修复痘痘、红点与轻微色斑",
    onValue: 45,
    group: "body",
  },
  {
    key: "bodyEven",
    name: "身体匀肤",
    caption: "减轻晒痕与局部肤色不均",
    onValue: 25,
    group: "body",
  },
  {
    key: "bodyBrighten",
    name: "身体提亮",
    caption: "提亮暗沉区域，保留衣物与背景",
    onValue: 15,
    group: "body",
  },
  {
    key: "bodyTexture",
    name: "身体纹理",
    caption: "轻度弱化疤痕、妊娠纹与干燥纹理",
    onValue: 30,
    group: "body",
  },
];

export const BASE_PORTRAIT_EFFECTS = PORTRAIT_EFFECTS.filter(
  (effect) => effect.group === "base",
);
export const BEAUTY_PORTRAIT_EFFECTS = PORTRAIT_EFFECTS.filter(
  (effect) => effect.group === "beauty",
);
export const FINE_PORTRAIT_EFFECTS = PORTRAIT_EFFECTS.filter(
  (effect) => effect.group === "fine",
);
export const WRINKLE_ZONE_EFFECTS = PORTRAIT_EFFECTS.filter(
  (effect) => effect.group === "wrinkle",
);
export const BODY_PORTRAIT_EFFECTS = PORTRAIT_EFFECTS.filter(
  (effect) => effect.group === "body",
);

export function isDefaultColor(params: ColorParams): boolean {
  return (Object.keys(DEFAULT_COLOR) as Array<keyof ColorParams>).every(
    (key) => params[key] === DEFAULT_COLOR[key],
  );
}

export function enabledPortraitEffects(params: PortraitParams): PortraitEffectSwitch[] {
  return PORTRAIT_EFFECTS.filter((effect) => params[effect.key] > 0);
}
