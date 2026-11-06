# IPC 契约 v1（前后端共同基线）

Tauri 命令使用单一参数 `req`，所有结构体在 Rust 端使用 `#[serde(rename_all = "camelCase")]`。
前端调用形式：`invoke<T>("command_name", { req })`。所有命令返回 `Result<T, ApiError>`。
路径字段以 `Path` 结尾，前端通过 `resolveAssetUrl(path)` 转换（Tauri 用 `convertFileSrc`，演示后端直接返回 `blob:`/`data:`）。

## 错误

```ts
interface ApiError {
  code: string;        // 稳定错误码，如 INVALID_IMAGE / RAW_UNSUPPORTED / PROVIDER_UNAVAILABLE
  message: string;     // 内部信息，可记录日志
  userMessage: string; // 面向普通用户的中文提示
}
```

## 应用与项目

```ts
interface AppInfo {
  version: string;
  backend: "cpu" | "wgpu";
  gpuAvailable: boolean;
  rawAvailable: boolean;
  faceModelAvailable: boolean;
  workspaceDir: string;
  supportedImageFormats: string[]; // ["jpg","jpeg","png","tif","tiff","webp"]
}

interface ProjectSummary {
  id: string;
  name: string;
  collectionId: string;     // 所属照片集；一张照片对应一个项目
  collectionName: string;
  width: number;
  height: number;
  updatedAt: string;        // ISO 8601
  thumbnailPath: string | null;
}

interface CollectionSummary {
  id: string;
  name: string;
  photoCount: number;
  updatedAt: string;              // 最新一张照片的更新时间
  coverThumbnailPath: string | null; // 最新一张照片的缩略图
  latestProjectId: string;        // 打开该照片集时默认展示的照片
}

interface CacheClearResult {
  deletedFiles: number;
  freedBytes: number;
}

interface SourceAssetState {
  assetId: string;
  displayName: string;
  width: number;
  height: number;
  format: string;            // "jpeg" | "png" | "tiff" | "webp"
  originalPath: string;
  previewPath: string;       // 最长边约 1600 的缓存 PNG
  thumbnailPath: string;     // 最长边约 480 的缓存 PNG
  colorSpace: string;        // 文件声明的色彩空间，如 "sRGB"、"Display P3"
  hasSourceProfile: boolean; // 导入时是否做过转换并保留了源配置
}

interface CanvasState {
  width: number;
  height: number;
  workingColorSpace: "linear-srgb";
}

interface CropState {
  x: number;      // 旋转后坐标系中的源图像像素
  y: number;
  width: number;
  height: number;
  angle: number;  // 顺时针旋转角度 -45..45，默认 0
}

type BlendMode = "normal" | "multiply" | "screen" | "overlay" | "softLight";
type LayerKind = "base" | "portrait" | "color" | "aiResult" | "retouch";

interface ColorParams {
  exposure: number;     // -3..3 EV，默认 0
  contrast: number;     // -100..100
  highlights: number;   // -100..100
  shadows: number;      // -100..100
  whites: number;       // -100..100
  blacks: number;       // -100..100
  temperature: number;  // -100..100
  tint: number;         // -100..100
  saturation: number;   // -100..100
  vibrance: number;     // -100..100
  clarity: number;      // -100..100
  sharpness: number;    // 0..100
  hue: number;          // -180..180 度，显示空间色相偏移
  hslSaturation: number; // -100..100，HSL 饱和度
  hslLightness: number;  // -100..100，HSL 明度
  curveX: number[];       // 5 个固定输入点，0..1
  curveY: number[];       // 5 个曲线输出点，0..1
  levelsInBlack: number;  // 0..0.9
  levelsInWhite: number;  // 0.1..1
  levelsGamma: number;    // 0.1..10
  levelsOutBlack: number; // 0..0.9
  levelsOutWhite: number; // 0.1..1
  lutAssetId: string | null; // `import_lut` 返回的资源 id，null 表示不使用 LUT
  lutStrength: number;       // 0..1，LUT 混合强度，默认 1
  noiseLuma: number;         // 0..100 亮度降噪（双边滤波）
  noiseChroma: number;       // 0..100 色彩降噪（色度模糊）
  vignette: number;          // -100..100 暗角，负值提亮四角
  grain: number;             // 0..100 胶片颗粒
  dehaze: number;            // 0..100 去雾（暗通道）
  chromatic: number;         // 0..100 紫绿边校正
}

interface PortraitParams {
  intensity: number;    // 0..100，总效果程度，默认 78（可在设置中调整预设程度）
  smooth: number;       // 0..100 磨皮
  texture: number;      // 0..100 细节保留，默认 70
  evenSkin: number;     // 0..100 肤色均匀
  brighten: number;     // 0..100 自然提亮
  bodySmooth: number;   // 0..100 身体磨皮
  // 精细修复，需要实测五官几何。几何由调用方从 analyze_portrait_regions
  // 的结果写入；缺失时后端按人脸椭圆比例派生，仍可生效。
  eyeEnhance: number;
  teethWhiten: number;
  blemish: number;
  wrinkle: number;
  darkCircle: number;
  mole: number;
  freckle: number;
  birthmark: number;
  tattoo: number;
  whiten: number;          // 0..100 自然美白
  whitenWarmth: number;    // -100 冷 .. 100 暖
  rosy: number;            // 0..100 红润
  eyeBag: number;          // 0..100 眼袋阴影
  wrinkleForehead: number; // 以下为分区细纹独立强度
  wrinkleGlabella: number;
  wrinkleCrowFeet: number;
  wrinkleNasolabial: number;
  wrinkleMouthCorner: number;
  wrinkleUnderEye: number;
  wrinkleNeck: number;
  bodyMode: boolean;       // true = 身体美颜图层，使用身体肤色范围
  bodyBlemish: number;
  bodyEven: number;
  bodyBrighten: number;
  bodyTexture: number;
  bodyWhiten: number;
  faceSlim: number;        // 0..100 局部塑形
  eyeSize: number;
  noseSlim: number;
  smoothRadius: number;    // 0..100，0 为自动磨皮半径
  whitenSaturation: number;// 0..100 美白通透（额外降饱和）
  oilControl: number;      // 0..100 去油光
  eyeWhite: number;        // 0..100 眼白提亮
  irisDetail: number;      // 0..100 虹膜细节
  teethBrighten: number;   // 0..100 在去黄之外提亮牙齿
  lipTint: number;         // 0..100 唇色
  lipWarmth: number;       // -100 冷/莓果 .. 100 暖/珊瑚
  noseContour: number;     // 0..100 鼻影
  eyeshadow: number;       // 0..100 上眼睑暖棕眼影
  browTint: number;        // 0..100 眉色加深
  jawWidth: number;        // 0..100 下颌线
  chinLength: number;      // -100 缩短 .. 100 拉长
  eyeDistance: number;     // -100 拉近 .. 100 拉开
  faceIndex?: number | null; // 该人像图层对应的检测人脸索引；null/缺失 = 旧版全局图层
  geometry?: PortraitGeometry | null; // 精细修复/塑形使用的五官椭圆，随图层持久化
}

interface PortraitEllipse {
  centerX: number; // 归一化源图坐标
  centerY: number;
  radiusX: number;
  radiusY: number;
  angle: number;   // 弧度
  feather: number; // 0..1
}

interface PortraitGeometry {
  face?: PortraitEllipse | null;
  leftEye?: PortraitEllipse | null;
  rightEye?: PortraitEllipse | null;
  leftBrow?: PortraitEllipse | null;
  rightBrow?: PortraitEllipse | null;
  mouth?: PortraitEllipse | null;
  teeth?: PortraitEllipse | null;
  neck?: PortraitEllipse | null;
}

interface LayerState {
  id: string;
  name: string;
  kind: LayerKind;
  visible: boolean;
  locked: boolean;
  opacity: number;          // 0..1
  blendMode: BlendMode;
  color: ColorParams | null;
  portrait: PortraitParams | null;
  retouch: RetouchParams | null;  // 局部修复图层
  mask: LayerMask | null;    // 手动画笔蒙版，仅调色/人像图层
  assetId: string | null;   // aiResult 图层
  previewPath: string | null;
  createdAt: string;
}

interface RetouchSpot {
  mode: "clone" | "heal";
  x: number;                  // 归一化源图坐标
  y: number;
  radius: number;             // 相对源图短边
  sourceX: number | null;     // 克隆取样点
  sourceY: number | null;
  hardness: number;           // 0..1，1 为硬边
  opacity: number;            // 0..1
}

interface LiquifyStroke {
  points: MaskPoint[];        // 归一化源图坐标，最多 1024 点
  radius: number;             // 相对源图短边，0.002..0.25
  strength: number;           // 0..1，再乘以图层不透明度
  mode: "push" | "pinch" | "bulge";
}

interface RetouchParams {
  spots: RetouchSpot[];       // 最多 400 笔，按顺序叠加
  warp: LiquifyStroke[];      // 最多 200 笔，先于修复点应用
  freeze: MaskStroke[];       // 最多 200 笔冻结保护；覆盖为 1 的像素不参与液化
}

interface MaskPoint {
  x: number;                // 相对原始源图归一化 0..1
  y: number;
}

interface MaskStroke {
  points: MaskPoint[];
  radius: number;           // 相对源图最短边 0.002..0.25
  feather: number;          // 0..1
  erase: boolean;
}

type MaskGradientKind = "linear" | "radial";

interface MaskGradient {
  kind: MaskGradientKind;
  start: MaskPoint;         // 线性：不透明端；径向：圆心
  end: MaskPoint;           // 线性：透明端；径向：半径端点
}

interface LayerMask {
  strokes: MaskStroke[];
  autoSkin: boolean;        // 启发式肤色范围，与画笔叠加
  gradient: MaskGradient | null; // 线性/径向渐变，可与画笔、肤色叠加
  inverted: boolean;        // 反转合成后的范围
  feather: number;          // 全局边缘羽化 0..1
  density: number;          // 整体修饰浓度 0..1
  regions: MaskRegion[];    // 五官区域椭圆，最多 32 个
}

interface MaskRegion {
  kind: string;             // 区域标识，如 "leftEye"、"skin"
  centerX: number;          // 相对原始源图归一化 0..1
  centerY: number;
  radiusX: number;          // 相对源图宽/高的半径
  radiusY: number;
  angle: number;            // 弧度，椭圆长轴旋转
  feather: number;          // 0..1，边缘过渡宽度
  strength: number;         // 0..1，该区域的最高不透明度
}

interface WhiteBalanceResult {
  temperature: number;      // -100..100，建议写入 ColorParams
  tint: number;             // -100..100
  sample: [number, number, number]; // 采样邻域平均 sRGB，供界面色块展示
}

interface AutoToneResult {
  exposure: number;         // -1..1 EV，带高光保护
  contrast: number;         // -10..15
  whites: number;           // -8..4
  blacks: number;           // -8..4
}

interface LutAssetResult {
  assetId: string;          // 写入 ColorParams.lutAssetId 的资源 id
  displayName: string;      // 文件名，仅用于界面显示
  dimension: "1D" | "3D";   // 由 cube 头部决定
  size: number;             // 1D 为条目数，3D 为单边格数
  fileBytes: number;
}

interface Histogram {
  red: number[];            // 64 个分箱
  green: number[];
  blue: number[];
  luma: number[];
}

interface DocumentState {
  id: string;
  name: string;
  collectionId: string;     // 照片集 id；旧文档载入时回填为项目 id
  collectionName: string;   // 照片集名称；旧文档载入时回填为项目名
  schemaVersion: number;    // 当前 13（照片集字段）
  revision: number;         // 单调递增，用于丢弃过期预览
  createdAt: string;
  updatedAt: string;
  canvas: CanvasState;
  crop: CropState;          // 非破坏性裁剪，渲染/导出时应用
  source: SourceAssetState | null;
  layers: LayerState[];     // 顺序即渲染顺序（从底到顶）
  canUndo: boolean;
  canRedo: boolean;
  history: string[];        // 可撤销动作标签（用于历史面板）
}
```

命令：

| 命令 | 请求 | 返回 |
| --- | --- | --- |
| `get_app_info` | 无 | `AppInfo` |
| `list_projects` | 无 | `ProjectSummary[]`（每张照片一条） |
| `list_collections` | 无 | `CollectionSummary[]`（按照片集分组，最新照片在前） |
| `create_project` | `{ name: string }` | `DocumentState`（空项目） |
| `open_project` | `{ projectId: string }` | `DocumentState` |
| `rename_project` | `{ projectId: string; name: string }` | `ProjectSummary`（只改照片名，不产生撤销记录） |
| `rename_collection` | `{ collectionId: string; name: string }` | `void`（重命名照片集内全部照片的 `collectionName`，不增加 revision、不产生撤销记录；打开中的项目同步更新） |
| `sync_color_adjustments` | `{ sourceProjectId: string; projectIds: string[] }` | `SyncColorAdjustmentsResult`（把源照片的调色图层复制到同集其他照片；目标调色图层整体替换，LUT 按项目复制；源项目、重复项与其他照片集的成员被跳过） |
| `delete_project` | `{ projectId: string }` | `void`（删除数据库记录和项目目录；有活动 AI 任务时拒绝） |
| `clear_render_cache` | 无 | `{ deletedFiles: number; freedBytes: number }`（只删除旧渲染预览） |
| `import_photo` | `{ path: string; collectionId?: string; collectionName?: string }` | `DocumentState`（创建照片项目并导入） |
| `import_photo_bytes` | `{ fileName: string; dataBase64: string; collectionId?: string; collectionName?: string }` | `DocumentState`（≤32MB，用于示例照片） |
| `import_lut` | `{ projectId: string; path?: string; fileName?: string; dataBase64?: string }` | `LutAssetResult`（校验并托管 `.cube`，不修改文档；`path` 用于桌面版，`dataBase64` 用于演示模式） |
| `render_preview` | `{ projectId: string; maxDimension?: number }` | `PreviewResult` |
| `update_layer_params` | `{ projectId: string; layerId: string; color?: ColorParams; portrait?: PortraitParams; retouch?: RetouchParams; blur?: BlurParams }` | `DocumentState`（不提交历史；`blur.radius` 为最短边比例 0..0.08，仅 `blur` 图层接受） |
| `update_layer_params_meta` | 同 `update_layer_params` | `DocumentState`（`layers` 恒为空数组，只用于合并 `revision` / 历史；拖动中每 ~70ms 的高频写入走这里，避免整份蒙版与修复点在 IPC 上往返） |
| `list_versions` | `{ projectId: string }` | `VersionSummary[]`（按创建时间倒序，最多 200 条；不含文档 JSON） |
| `create_version` | `{ projectId: string; name: string }` | `VersionSummary[]`（保存当前文档快照；名称裁剪空白后 1..60 字符；单条 JSON 超过 4 MiB 返回 `INVALID_ARGUMENT`；每项目超过 50 个或累计 32 MiB 时丢弃最旧，并一并删除其预览缓存。最新一条永不淘汰） |
| `restore_version` | `{ projectId: string; versionId: string }` | `DocumentState`（恢复快照；项目 id / 照片集 / 照片名保持不变，revision 前进，并写入一条可撤销历史「恢复版本：名称」） |
| `delete_version` | `{ projectId: string; versionId: string }` | `VersionSummary[]`（按 projectId 限定，越权 id 视为不存在；同时删除该快照的预览缓存） |
| `render_version_preview` | `{ projectId: string; versionId: string; maxDimension?: number }` | `PreviewResult`（把快照文档渲染成 PNG 供并排对比；缓存键为 `version-{versionId}-d{dim}.png`，不参与 `render-*` 的轮换） |
| `check_segmentation_capability` | `{ modelPath?: string }` | `SegmentationCapability`（加载模型并用 512×512 空图实跑一次；结果按模型路径缓存，未配置/路径不存在/契约不符都会返回 `available: false` 与原因） |
| `run_segmentation_mask` | `{ projectId: string; layerId: string; modelPath: string; threshold?: number; softness?: number }` | `DocumentState`（在 1600px 代理上推理，结果写成 `mask-{id}.png` 资产并挂到图层，提交历史「智能蒙版」） |
| `clear_raster_mask` | `{ projectId: string; layerId: string }` | `DocumentState`（只移除栅格覆盖，保留画笔/渐变；没有栅格时不产生历史） |
| `commit_changes` | `{ projectId: string; label: string }` | `DocumentState`（将本次连续修改合并为一条历史） |
| `set_layer_mask` | `{ projectId: string; layerId: string; mask: LayerMask \| null }` | `DocumentState`（仅调色/人像图层；含画笔、自动肤色、渐变、反转、羽化与密度；每次调用提交一条历史） |
| `pick_white_balance` | `{ projectId: string; x: number; y: number }` | `WhiteBalanceResult`（x/y 为原始源图归一化坐标，只读采样，不修改文档） |
| `auto_tone` | `{ projectId: string }` | `AutoToneResult`（基于源图亮度直方图；只读建议，不修改文档） |
| `set_crop` | `{ projectId: string; crop: CropState }` | `DocumentState`（写入裁剪与角度并提交一条历史；角度非零时导出会旋转完整画面后再裁剪） |
| `add_layer` | `{ projectId: string; kind: "portrait" \| "color" \| "retouch" \| "blur"; name?: string; portrait?: PortraitParams; mask?: LayerMask \| null }` | `DocumentState`（可选初始参数/蒙版用于一次历史记录创建人脸美颜层；非人像图层携带 `portrait` 或不可蒙版图层携带 `mask` 会被拒绝；`blur` 图层默认半径 0.012，支持画笔与栅格蒙版） |
| `update_layer` | `{ projectId: string; layerId: string; patch: { name?: string; visible?: boolean; locked?: boolean; opacity?: number; blendMode?: BlendMode } }` | `DocumentState`（仅不透明度的补丁不提交历史；其余字段立即提交） |
| `duplicate_layer` | `{ projectId: string; layerId: string }` | `DocumentState`（复制参数与修饰范围，插入原图层上方并提交一条历史） |
| `delete_layer` | `{ projectId: string; layerId: string }` | `DocumentState` |
| `move_layer` | `{ projectId: string; layerId: string; direction: "up" \| "down" }` | `DocumentState` |
| `undo` | `{ projectId: string }` | `DocumentState` |
| `redo` | `{ projectId: string }` | `DocumentState` |
| `export_image` | `{ projectId: string; options: ExportOptions; jobId?: string }` | `ExportResult`（提供 jobId 时推送导出进度并可取消） |
| `cancel_export` | `{ jobId: string }` | `boolean`（任务不存在或已完成时返回 false） |
| `analyze_portrait` | `{ projectId: string }` | `PortraitAnalysisResult`（内置 CenterFace + YuNet，ONNX 经 tract 本地推理；两模型交叉验证并合并框，人脸框换算回源图像素；`confidence` 为 0..1，`models` 标明来源） |
| `analyze_portrait_regions` | `{ projectId: string }` | `PortraitRegionsResult`（在人脸框内实测五官椭圆区域与关键点；在 1600px 分析代理上测量并按代理尺寸归一化，结果按打开的项目缓存） |
| `run_ai_denoise` | `{ projectId: string; modelPath: string; strength?: number }` | `DocumentState`（用用户自备的本地 ONNX 模型分块降噪，写入 `ai_result` 资产并新增“AI 降噪”图层；`strength` 0..1，默认 1） |
| `run_photo_hq_denoise` | `{ projectId: string; strength?: number; jobId?: string }` | `DocumentState`（用内置 Photo-HQ-W4xEX / Real-ESRGAN 4x 在**本机 GPU** 推理，4x 结果缩回源尺寸并混合，新增“AI 降噪 · Photo-HQ”图层；`strength` 0..1，默认 1；提供 `jobId` 时可通过 `cancel_ai_denoise` 取消） |
| `cancel_ai_denoise` | `{ jobId: string }` | `boolean`（向正在运行的 Photo-HQ 引擎发送取消；引擎在 250ms 轮询内被杀掉，不写入图层，返回 `AI_DENOISE_CANCELLED`） |
| `check_ai_denoise_capability` | `{ onnxModelPath?: string \| null }` | `AiDenoiseCapability`（`photoHq` 用 32×32 实跑自检并缓存结果，`onnx` 检查文件是否存在；各自返回 `available` 与不可用原因） |

`create_project` 创建空项目（无 source、无图层），仅用于测试/占位。
`import_photo` 与 `import_photo_bytes` 始终创建一张新的照片项目：传 `collectionId` 加入现有照片集，传 `collectionName` 新建照片集；两者都省略时，这张照片自成一个照片集。已存在项目通过 `open_project` 打开。照片集没有独立数据表，`list_collections` 把照片按 `collectionId` 分组，因此空照片集不存在，删除照片集的最后一张照片即删除该照片集。

```ts
interface RenderPreviewRequest {
  projectId: string;
  maxDimension?: number;
  ignoreCrop?: boolean;   // 裁剪工具需要完整画面；缓存键包含该标记
}

interface PreviewResult {
  path: string;        // 缓存 PNG 路径
  width: number;
  height: number;
  revision: number;
  durationMs: number;
  cached: boolean;
  backend: "wgpu" | "cpu" | "cache"; // 实际渲染后端
  histogram: Histogram | null;       // 预览 RGB/亮度分布，缓存命中也返回
}

interface ExportProgress {
  jobId: string;
  phase: "preparing" | "rendering" | "encoding" | "completed" | "cancelled";
  percent: number | null;   // 0..100
  message: string;
}

interface ExportOptions {
  path: string;
  format: "jpeg" | "png" | "tiff" | "webp";
  quality: number;              // 1..100，PNG/TIFF 忽略
  maxDimension: number | null;  // null = 原始分辨率
  width: number | null;         // 与 height 同时提供；按封面裁切到精确尺寸
  height: number | null;
  colorSpace: "srgb" | "displayP3" | "source"; // 缺省 "srgb"
  metadata: "keep" | "privacy" | "none";      // 缺省 "keep"
  bitDepth: "eight" | "sixteen";              // 缺省 "eight"，16 位仅 PNG/TIFF
  watermark?: WatermarkOptions | null;        // 缺省 null；仅写入导出文件
}

interface WatermarkOptions {
  imageBase64: string;          // PNG/JPEG data URL 或纯 base64；文字由前端光栅化
  opacity: number;              // 0..1，叠加在图片自身 alpha 之上，缺省 0.6
  scale: number;                // 水印宽度 / 导出短边，0.01..1，缺省 0.18
  margin: number;               // 边距 / 导出短边，0..0.5，缺省 0.04
  position: "topLeft" | "topRight" | "bottomLeft" | "bottomRight" | "center"; // 缺省 bottomRight
}

interface ExportResult {
  path: string;
  bytes: number;
  width: number;
  height: number;
  durationMs: number;
  backend: "wgpu" | "cpu";
  colorSpace: string;           // 实际写入的空间，可能回退为 "sRGB"
  bitDepth: number;             // 8 或 16
  metadataWritten: boolean;     // EXIF 是否真的写入（TIFF 会读回文件确认）
  keepsLocation: boolean;       // 写入的 EXIF 是否仍含 GPS 位置
}

interface PortraitWarning {
  code: string;
  message: string;
}

interface PortraitAnalysisResult {
  available: boolean;
  faces: Array<{ faceId: string; x: number; y: number; width: number; height: number; confidence: number }>;
  warnings: PortraitWarning[];
}

interface PortraitRegion {
  kind: string;             // "skin" | "leftEye" | "rightEye" | "leftBrow" | "rightBrow" | "mouth" | "lips" | "teeth" | "nose"
  label: string;            // 中文标签，供界面直接展示
  centerX: number;          // 相对原始源图归一化 0..1
  centerY: number;
  radiusX: number;          // 相对源图宽/高
  radiusY: number;
  angle: number;            // 弧度
  feather: number;          // 建议羽化 0..1，可直接写入 MaskRegion
  confidence: number;       // 0..1，实测证据强度；低于阈值的面部部件会被省略
}

interface PortraitKeypoint {
  kind: string;
  label: string;
  x: number;                // 归一化 0..1
  y: number;
}

interface PortraitRegionsResult {
  available: boolean;
  faces: Array<{
    faceId: string;
    box: { faceId: string; x: number; y: number; width: number; height: number; confidence: number }; // 像素坐标
    keypoints: PortraitKeypoint[];
    regions: PortraitRegion[];
    score: number;          // 0..1，整张脸的部件覆盖率加权置信度
  }>;
  warnings: PortraitWarning[];
}
```

## AI Provider

```ts
type ProviderKind = "mock" | "openaiCompatible";

interface ProviderCapabilities {
  supportsMask: boolean;
  supportsReferenceImages: boolean;
  supportsMultipleImages: boolean;
  returnsImage: boolean;
  supportsStructuredParameters: boolean;
  supportsCancellation: boolean;
  maxWidth: number | null;
  maxHeight: number | null;
  maxInputBytes: number | null;
}

interface ProviderInfo {
  id: string;
  name: string;
  kind: ProviderKind;
  baseUrl: string | null;
  model: string | null;
  hasCredential: boolean;
  capabilities: ProviderCapabilities;
  createdAt: string;
}

interface ProviderConfigInput {
  id?: string | null;
  name: string;
  kind: ProviderKind;
  baseUrl?: string | null;
  model?: string | null;
  allowLocalhost?: boolean;
}

interface ProviderTestResult {
  ok: boolean;
  latencyMs: number;
  message: string;
}

type AiJobStatus = "queued" | "running" | "completed" | "failed" | "cancelled";

interface AiJobInfo {
  id: string;
  projectId: string;
  providerId: string;
  providerName: string;
  operation: "edit" | "suggest";
  prompt: string;
  status: AiJobStatus;
  progress: number | null;   // 0..100
  message: string | null;
  resultAssetId: string | null; // 已完成结果的资产 ID，与图层 assetId 对应；不等于 jobId
  resultPath: string | null;
  errorCode: string | null;
  userMessage: string | null;
  createdAt: string;
  updatedAt: string;
}
```

命令：

| 命令 | 请求 | 返回 |
| --- | --- | --- |
| `list_providers` | 无 | `ProviderInfo[]`（首次启动自动包含 Mock 演示 Provider） |
| `save_provider_config` | `ProviderConfigInput` | `ProviderInfo` |
| `set_provider_credential` | `{ providerId: string; apiKey: string }`（空串清除） | `ProviderInfo` |
| `test_provider` | `{ providerId: string }` | `ProviderTestResult` |
| `list_provider_models` | `{ providerId: string }` | `string[]`（OpenAI 兼容服务读取 `/models`；Mock 返回内置模型） |
| `submit_ai_job` | `{ projectId: string; providerId: string; operation: "edit" \| "suggest"; prompt: string; strength: number }` | `{ jobId: string }` |
| `cancel_ai_job` | `{ jobId: string }` | `void` |
| `list_ai_jobs` | `{ projectId: string }` | `AiJobInfo[]` |
| `apply_ai_result` | `{ projectId: string; jobId: string }` | `DocumentState`（新增 aiResult 图层） |

事件：`ai-job-updated`，负载为 `AiJobInfo`。前端使用 `listen<AiJobInfo>("ai-job-updated", ...)`。
Mock Provider 完全本地执行，不上传图片；OpenAI 兼容 Provider 仅在用户勾选上传确认后才能提交。

## 行为约束

- 原图不复制、不修改；项目记录原始路径，源文件缺失时给出用户提示。
- `rename_project` 只改单张照片的名称，不改变所属照片集的名称，不增加 revision、不产生撤销记录；`delete_project` 会删除照片项目、资产与缓存，不可撤销，存在活动 AI 任务时返回错误。
- 照片集由照片上的 `collectionId` / `collectionName` 派生：新建照片时传 `collectionName` 创建集合，传 `collectionId` 加入现有集合。数据库 schema v2 给旧项目回填 `collection_id = id`、`collection_name = name`，因此旧数据升级后每张照片各自成为一个照片集；文档 schema 13 缺字段时同样按项目 id/name 回填。
- 批量导出不是独立 IPC：前端对当前照片集中勾选的照片依次调用 `export_image`，沿用当前导出格式/质量与各自裁剪后的原尺寸，先 `settlePendingEdits` 再逐个导出，重名时自动追加序号。该流程仅桌面版提供目录选择。
- 批量导入同样不是独立 IPC：多选文件（文件对话框或一次拖入多张）后前端顺序调用 `import_photo`（无当前项目时第一张创建照片集，其余沿用返回的 `collectionId`），每张完成即刷新进度；单张失败只计入汇总，不中断整批。导入前先 `settlePendingEdits`，避免切换文档时丢失未提交的滑杆调整。
- 批量同步调色由 `sync_color_adjustments` 完成：只接受与源项目同一照片集的成员，源项目与重复 id 会被跳过，跨集成员计入 `failed`。目标原有的调色图层会被源图层的副本整体替换（保留图层名、混合模式、不透明度、可见性与归一化蒙版），插入位置沿用原调色图层的位置；没有调色图层时插在背景图层之后。每个目标各写入一条「同步调色调整」历史，可单独撤销。LUT 文件复制为目标项目自己的资产，源项目删除后目标仍可渲染；LUT 文件已丢失时只同步其余参数。裁剪、人像（含人脸几何）与局部修复依赖源照片的构图和五官位置，不同步。
- 每次 `import_photo` 解码前校验格式、尺寸（≤100MP）与文件大小（位图 ≤32MB，RAW ≤200MB）。
- RAW 使用 rawler 基础解码，覆盖松下 RW2/RWL、尼康 NEF/NRW、索尼 ARW 等格式；解码后从 RAW 元数据读取 EXIF Orientation 并应用到像素，画布尺寸、预览、裁剪与检测坐标都以用户看到的方向为准。打开旧 RAW 项目时，如果画布仍是横置尺寸且没有坐标类编辑（全幅裁剪、无蒙版/修复/AI 结果图层），会自动转正并重建预览与缩略图；带编辑的旧项目不做自动迁移，建议重新导入。已验证松下 DC-FZ80、尼康 1 J1、索尼 DSC-RX100 的导入/预览/导出，其他机型取决于相机数据库。Provider 不接收 RAW，AI 任务使用生成的 sRGB 预览。
- `analyze_portrait` 在 1600px 分析代理上缩到 800px，再分别运行 CenterFace 与 YuNet（MIT，ONNX/tract）。两模型框 IoU > 0.45 时合并为一个，置信度取平均并保留 `centerface` / `yunet` 来源；只被一个模型看到的框需达到 0.7 才保留。返回坐标换算回分析图像素，再由调用方缩放为源图像素。导入方向已校正，检测侧不做四方向扫描。
- `analyze_portrait_regions` 在 1600px 分析代理上测量五官，归一化必须使用代理宽高，再映射为源图的 0..1 坐标；不能用源图像素直接除代理像素。区域结果按项目缓存；同一项目已由 `analyze_portrait` 得到的人脸框也会缓存并按尺寸复用，因此打开人像面板不会重复跑 ONNX。
- 人脸编号按源图从左到右排序（同列再按纵向），保证同一张照片重复检测时 `faceIndex` 稳定。`confidence` 为 0..1 的双模型合并分数，前端直接显示为百分比；它不是经过校准的概率。
- 逐脸美颜由前端编排：应用预设或效果时为所选脸调用 `add_layer`（带 `name`、`portrait.faceIndex`、`portrait.geometry` 和 `kind = "face"` 的椭圆蒙版），同一 `faceIndex` 已有图层则复用。`faceIndex` 表达编辑归属；蒙版决定像素作用范围，几何决定眼睛、牙齿等精细修复的落点。旧版全局人像层 `faceIndex = null`，单脸照片继续复用。文档 schema 16 增加 `geometry`、肤色美颜、分区细纹、身体美颜、面部塑形与修复图层 `warp` 字段；schema 17 增加 `bodySlim`，schema 18 增加 `bodyLengthen` 与 `RetouchParams.freezeGrid`（16×16 布尔网格，按全画面行优先排列）。缺失时按默认值读取。`bodySlim` / `bodyLengthen` 只在人像图层蒙版内生效：渲染端把蒙版栅格化为覆盖率，向覆盖率加权重心水平收窄、对重心以下做垂直拉伸，权重在蒙版边缘与人脸椭圆内降为零；没有蒙版时不产生位移。
- 细分参数：`smoothRadius` 只缩放磨皮核大小，0 保持自动；`whitenSaturation` 在美白中额外降饱和形成通透感；`oilControl` 只压缩肤色掩膜内的高光低饱和区域；`eyeWhite` 与 `irisDetail` 分别作用于眼白和虹膜，`eyeEnhance` 作为总开关同时驱动两者；`teethBrighten` 在牙齿门限内只提亮不额外去黄；`lipTint` / `lipWarmth` 在唇部椭圆内加色并避开牙齿；`noseContour` 在鼻翼两侧加阴影、鼻梁轻提亮；`eyeshadow` 在上眼睑椭圆内加暖棕，并用亮度门限避开睫毛与眼球；`browTint` 在眉部椭圆内加深并加暖。
- 肤色美颜：`whiten` 在肤色掩膜内提亮、降低饱和度并按 `whitenWarmth` 偏冷/偏暖；`rosy` 以脸颊椭圆为重心加暖红；`eyeBag` 与 `darkCircle` 共用一张眼下权重图，避免同时开启时重复提亮。三者都乘五官保护掩膜，不漂白眼睛、嘴唇或牙齿。
- 分区细纹：`wrinkle` 是总开关，`wrinkleForehead` / `wrinkleGlabella` / `wrinkleCrowFeet` / `wrinkleNasolabial` / `wrinkleMouthCorner` / `wrinkleUnderEye` / `wrinkleNeck` 可独立设置；分区值只会抬高、不会降低总开关。多个分区共用一张权重图，重叠处只处理一次。颈纹需要身体美颜层或包含颈部蒙版的人像层。
- 身体美颜：`bodyMode = true` 的人像层使用加色度门限的身体肤色掩膜（灰墙、黑衣、白衣不会进入掩膜），并按几何排除当前选中的人脸；`bodySmooth` / `bodyWhiten` / `bodyBlemish` / `bodyEven` / `bodyBrighten` / `bodyTexture` 各自独立。身体层不需要人脸索引，但有人脸几何时颈纹与面部排除更准确。
- 面部塑形：`faceSlim` / `jawWidth` / `chinLength` / `eyeDistance` / `eyeSize` / `noseSlim` 在颜色处理之前对合成画面做逆向双线性采样；位移权重在脸缘前收敛到零，背景直线不会被硬切断。`chinLength` 与 `eyeDistance` 是有符号参数，其余为 0..100。塑形只依赖几何，不生成新图层；关闭图层或把参数归零即恢复原状。
- 组合预设：前端只把预设展开成一组普通 `PortraitParams` 写入图层，不引入不可逆滤镜；应用后每个子效果仍可单独关闭、归零或撤销。内置「自然美颜」「清透提亮」「轻度精修」「奶油肌」「氧气感」「舞台妆」。
- 手动液化：`RetouchParams.warp` 按顺序应用推拉/收缩/膨胀笔触；每个笔触先取局部快照再逆向采样，位移上限为笔刷半径的 0.9 倍，长拖动不会把自己的输出反馈回输入。`RetouchParams.freeze` 是冻结保护笔触，渲染时先栅格化为覆盖图，液化位移乘以 `1 - coverage`。液化与修复点在同一「局部修复」图层内，先液化后修复点，含该图层时回退 CPU。
- 精细修复（`eyeEnhance` / `teethWhiten` / `darkCircle` / `wrinkle` / `blemish` / `mole` / `freckle` / `birthmark` / `tattoo`）在 CPU 管线中按几何局部处理：眼睛用眼周局部反锐化提亮（保护瞳孔），牙齿用高亮低饱和门限去黄（不漂白嘴唇），黑眼圈提亮眼下阴影并向肤色中性化，皱纹只压制额头/眉间/法令纹/眼角的暗线，斑点修复用多尺度模糊参考压制高反差缺陷并避开眼睛、眉毛、嘴与牙齿。没有几何时只保留斑点修复的肤色回退；打开任一需要五官的效果时前端会先触发一次区域测量，测量完成后自动升级已启用精细修复的图层几何。所有效果仍受图层蒙版、不透明度与整体程度约束。
- 精细修复没有分割网络，几何是带置信度的椭圆；侧脸、遮挡、浓妆或张嘴幅度过大时会定位偏差。大地胎记/纹身只能明显淡化，不能保证完全无痕；手动液化属于「局部修复」图层，不在精细修复的颜色管线内。
- 裁剪坐标相对旋转后的外接画面；`render_preview` 与 `export_image` 在图层合成后应用裁剪。前端裁剪工具把选框限制在旋转后图片的安全内接区域内（并限制输出不超过原图宽高），旋转时自动内缩，因此新裁剪不会包含透明角；后端仍按文档保存既有矩形，以保持旧数据不变。导出宽高比例不一致时居中裁切，不拉伸。
- `pick_white_balance` 在原始源图上取邻域平均，跳过接近纯黑或过曝的像素后求解冷暖/色调；裁剪不影响坐标语义。该命令只返回建议值，前端将其写入调色图层后通过 `commit_changes` 提交“白平衡吸管”历史。
- `auto_tone` 对源图抽样生成亮度直方图，按 2% / 50% / 98% 分位给出曝光、对比与黑白场建议；正曝光不超过高光余量，避免把肤色推入过曝。前端写入调色图层后提交“自动校正”历史。
- 导出元数据：`keep` 保留源 EXIF，但会把方向标签改写为 1（方向已在解码时烘焙进像素）并删除过期的内嵌缩略图；`privacy` 在此基础上再删除 GPS 指针；`none` 完全不写。源文件本身永不被修改，元数据只在导出时读取。TIFF 的 EXIF 在编码后追写 IFD0/ExifIFD，导出后读回文件确认标签存在，写不进去时以 `metadataWritten: false` 如实回报。XMP 当前只读不写。
- 16 位导出：`bitDepth = "sixteen"` 时跳过 GPU 路径（GPU 输出 8 位像素），在 CPU 线性管线中渲染、缩放后再量化到 16 位 sRGB，写出 RGBA16 的 PNG/TIFF；JPEG 请求 16 位返回 `BIT_DEPTH_UNSUPPORTED` 且不留文件。
- 导入时读取文件内嵌 ICC 配置：识别 sRGB / Display P3 / Adobe RGB / 其他 RGB（矩阵 + TRC 类型），并把像素转换到线性 sRGB 工作空间；原文件不改动，转换只发生在缓存与渲染中。超出 sRGB 的像素会被截断，因此 `hasSourceProfile` 的照片导出时可选 `source` 写回原空间。
- 导出色彩空间：`srgb` 不写配置（通用约定），`displayP3` 写入内置生成的 P3 配置，`source` 写回导入时保留的配置。`ExportResult.colorSpace` 表示实际写入值，照片没有可保留配置时 `source` 会回退为 `sRGB`，界面据此提示。
- TIFF 导出写入 `IccProfile` 标签；其解码器读不回 BYTE 数组，因此导入 TIFF 时直接读取该标签补全色彩空间，避免导出再导入时被误判为 sRGB。基于 3D LUT 的配置（`mft1`/`mft2`）与 CMYK 只识别不转换。
- `render_preview.histogram` 始终基于最终渲染结果计算；缓存命中时直接返回记忆的直方图，不写入项目数据。
- 效果类参数（降噪、去雾、色差）在颜色管线前段执行，暗角与颗粒在颜色与锐化之后执行；任一非零都会让该文档回退 CPU 渲染。
- `import_lut` 只把 `.cube` 校验（扩展名、≤8MB、1D 2..256 / 3D 2..64、条目数完整）后复制到项目资产目录并登记为 `kind = "lut"` 的资源，不修改文档；套用与移除都通过 `update_layer_params` 写 `lutAssetId`，因此与其他调色参数一样可撤销、可复制粘贴。LUT 在显示 sRGB 空间采样并按 `lutStrength` 混合，`lutStrength = 0` 时不读取资产文件。资产缺失或损坏返回错误码 `LUT_INVALID`，不会静默跳过。
- `export_image` 提供 `jobId` 时通过 Tauri 事件 `export-progress` 推送阶段进度（准备 / 渲染 / 编码 / 完成 / 取消）。导出在阶段边界检查取消令牌；编码已落盘后才收到取消时，只会删除本次新建的文件，不覆盖用户原有文件。取消错误码为 `EXPORT_CANCELLED`。
- 导出水印只作用于输出文件：前端把文字画成透明 PNG（或读取用户 Logo），再以 `watermark` 随 `ExportOptions` 传入；后端在输出尺寸确定后、ICC 转换前完成缩放与 alpha 合成，8/16 位与 GPU/CPU 路径一致。水印图片上限 8MB / 4096px，无效或超限返回 `INVALID_ARGUMENT` 且不会留下半成品；批量导出沿用同一份水印设置。文档、图层与原图都不记录水印。
- `update_layer_params` 只改内存文档；`commit_changes` 才生成一条撤销记录并触发自动保存。滑块拖动必须使用该组合。
- `update_layer` 中仅含 `opacity` 的补丁同样是为拖动服务，不生成历史；前端松开滑块后调用 `commit_changes`。可见性/混合模式等离散补丁立即生成一条历史。
- `render_preview.maxDimension` 由前端设置页决定（1200/1600/2048），后端上限 8192；缓存键包含 revision 与该尺寸。
- 字段名由 `crates/app-core/tests/contract.rs` 锁定。前端 `types.ts` 是手写镜像，重命名字段或漏掉 `rename_all = "camelCase"` 会让该测试失败；新增字段时同步改测试与本文档。
- `set_layer_mask` 收到不含 `raster` 的蒙版时会**保留**已有栅格覆盖（画笔/渐变界面不发送该字段）；需要删除栅格时调用 `clear_raster_mask`，清空整个蒙版仍会一并移除。
- `*_version` 系列的成功响应只保证 `DocumentState` 的语义；`restore_version` 返回的文档已包含快照内容，调用方应整体替换当前文档而不是局部合并。版本快照存于 SQLite，删除照片时随外键级联清理。
- 拖动滑块时前端请求 900px 的交互档位，静止约 420ms 后自动回到设置档位；两档写入各自的缓存键，最终像素与导出不受影响。`update_layer_params_meta` 的响应不可当作文档使用。
- 图层顺序即渲染顺序；base 图层不可删除、不可调整参数。`duplicate_layer` 复制除 base 外的任意图层，参数与蒙版一并复制，副本默认解锁。
- 跨照片复用参数由前端完成：只复制调色或人像参数，不携带蒙版、裁剪、五官几何或 AI 资产；粘贴时按当前照片重新解析几何，为对应调整图层创建/复用并提交历史。`bodyMode` 跟随目标图层，不会因粘贴把人像层意外切成身体层。
- 不支持 AI 人脸分析时，`analyze_portrait` 与 `analyze_portrait_regions` 返回 `available: false` 与警告，不得报 500。
- 预览渲染使用代理图，导出必须基于原始分辨率；缓存键至少包含 projectId、revision、maxDimension。
- WGPU 为可选加速路径：基础照片、调色（清晰度/锐度、HSL、曲线/色阶、LUT 必须为默认值）、裁剪与 AI 结果图层可走 GPU；含活动人像图层、局部修复/液化图层、清晰度/锐度、HSL、曲线/色阶、生效的 LUT 或画笔/渐变蒙版时自动回退 CPU。实际后端通过 `backend` 字段返回。
- 画笔、渐变与自动肤色坐标均相对原始源图保存，裁剪后仍能正确映射；蒙版只作用于对应调色/人像图层的调整。
- `autoSkin` 为启发式肤色范围（YCbCr 权重 + 羽化），不是 AI 语义分割。渐变与画笔、肤色按覆盖并集合成，再应用全局羽化、反转与浓度；含活动蒙版时预览/导出回退 CPU。
- `analyze_portrait_regions` 是纯本地的实测证据（YCbCr 肤色轨迹 + 暗核亮环斑点 + 红度矩），输出带置信度的椭圆区域，不是逐像素语义分割；检不出来的部件直接省略，绝不猜测。结果按打开的项目缓存，检出的椭圆可由前端写入 `LayerMask.regions` 成为真实蒙版，最多 32 个。
- `run_ai_denoise` 只接受用户自备的 `.onnx` 文件（桌面版设置中选择，浏览器演示没有本地推理）；输入固定为 `[1,3,192,192]` 的 NCHW float（sRGB 0..1），输出必须保持 192×192，因此只支持去噪、不支持超分放大。推理由纯 Rust `tract` 在本机 CPU 执行，192px 分块、24px 重叠并用羽化权重融合；`strength` 在原图与模型输出之间线性混合，alpha 保持不变。结果 PNG 登记为 `ai_result` 资产并新增 AI 结果图层，源图与已有图层不受影响。模型加载与运行错误分别返回 `AI_DENOISE_MODEL_INVALID` / `AI_DENOISE_FAILED`；ONNX 通道的模型由用户提供，不随应用分发。
- `run_photo_hq_denoise` 调用随应用打包的 `realesrgan-ncnn-vulkan`（MIT）与 Photo-HQ-W4xEX 权重，固定参数为 `-n Photo-HQ-W4xEX -s 4 -t 192 -f png`，模型目录使用相对于引擎目录的 `models`。引擎定位顺序为 `LUMA_REALESRGAN_DIR` → `%LOCALAPPDATA%/<bundle-id>/engines/realesrgan` → 打包资源目录。输入最长边超过 1600px 时先缩小，4x 输出再用 Lanczos 缩回源尺寸并按 `strength` 混合，alpha 保留。需要支持 Vulkan 的 GPU；无 Vulkan 时返回 `AI_DENOISE_FAILED`，不会退化为 CPU。子进程有 600 秒超时，stdout/stderr 写入临时日志，失败时把日志尾部用于诊断。传入 `jobId` 后任务注册到统一任务表，`cancel_ai_denoise` 置位取消令牌，引擎轮询在 250ms 内终止子进程并返回 `AI_DENOISE_CANCELLED`，不会生成图层或资产；前端同时显示已用时间。引擎文件缺失返回 `AI_DENOISE_ENGINE_MISSING`。结果图层名称为“AI 降噪 · Photo-HQ”。
- `check_ai_denoise_capability` 是 AI 总开关的预检：`photoHq` 会先校验引擎与模型文件，再用 32×32 切块（`-t 32`）真实跑一次推理，覆盖模型加载、Vulkan 初始化与路径处理，并在服务进程内缓存结果；`onnx` 只检查 `onnxModelPath` 是否存在。两项都不可用时前端禁用 AI 选项并说明原因，降噪保持普通模式；如果预检通过但正式运行失败，前端也会自动回退普通降噪。AI 结果图层的强度通过图层不透明度表达，关闭 AI 时隐藏图层并恢复普通降噪参数。
- AI 原始响应必须校验后才能落盘/建模；AI 结果只作为新图层，不覆盖源图。
- OpenAI 兼容 Base URL 支持粘贴完整端点：结尾为 `/images/edits`、`/images/generations` 或 `/models` 时会归一化为 API 根，再拼接实际请求路径。localhost Provider 的 HTTP 客户端使用 `no_proxy` 直连；远程 Provider 遵循系统 `HTTPS_PROXY`/`ALL_PROXY`，未配置代理时请求前会解析域名并拒绝私有/链路本地结果。重定向仍然禁止。
- 客户端在切换照片后必须丢弃旧项目的异步结果：AI 降噪、应用 AI 结果、自动校正、白平衡、LUT 导入与五官区域识别的回写都以项目 id 校验，不能写入新打开的项目。
