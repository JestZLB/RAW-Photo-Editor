# ADR-003：项目文档、图层与编辑历史模型

- 状态：已接受
- 日期：2026-10-20
- 关联：架构文档 05，IPC 契约 v1

## 背景

需要非破坏式编辑：原图不覆盖、参数可回放、AI 结果可审查。文档不能携带像素缓冲，历史不能为每一帧滑块生成一条撤销记录。

## 决策

1. `ProjectDocument`（schemaVersion = 13）保存：画布、非破坏性裁剪矩形、源资产引用（含色彩空间标签与 ICC 配置引用）、图层数组（含调色/人像画笔、渐变、自动肤色蒙版、五官区域椭圆及其反转/羽化/浓度选项、HSL、曲线与色阶、LUT 资源引用与强度）、所属照片集（`collectionId` / `collectionName`）、时间戳与单调递增的 `revision`。
2. 图层即编辑单元：`base`、`portrait`、`color`、`aiResult`。参数使用强类型结构，不接受任意 JSON 进入渲染器。
3. 图层顺序即渲染顺序；base 固定在底部，不允许删除或重排到其他图层之上。
4. 历史保存在会话中（`DocumentSession`）：连续滑块修改先改变文档，`commit(label)` 时把修改前快照压入撤销栈，确保一次拖动只产生一条记录；快照只包含文档 JSON，不包含像素。
5. 持久化：文档 JSON 存入 SQLite，源图仅记录路径；`import_photo_bytes` 的示例/内存导入会落盘一份源文件。大图、预览、AI 结果作为独立资产保存。
6. 落盘在会话锁之外：命令在锁内只修改文档并标记"待保存"，`session()` 释放全局锁后再克隆快照写 SQLite（实测 6 MB 的蒙版文档要序列化 44 ms，放在锁内会连带阻塞其他项目的预览）。写入由独立的 `save_lock` 串行化，每个项目带单调 `save_seq` / `last_saved_seq`：并发下若较新的快照先落盘，较旧的快照会被跳过，绝不回退。逐层缩略图的 `exists()` 探测仍在锁内（单次约 10 µs，可以忽略）。窗口关闭前前端先冲刷防抖队列并提交一次（Tauri 的 `onCloseRequested` 阻止关闭直到完成，3 秒超时兜底），因此未松手的滑块调整也会落盘；写入失败时状态栏显示「保存失败」而不是假报已保存。
7. LUT 只以资源 id 进入文档（`lutAssetId` + `lutStrength`），`.cube` 文本作为 `kind = "lut"` 资产存在项目目录内；文档不内联 LUT 数据，删除项目即清理 LUT。旧版本文档缺少这两个字段时按 `serde(default)` 读取为“无 LUT”。
8. 效果参数（schema 12）：`ColorParams` 新增 `noise_luma` / `noise_chroma` / `vignette` / `grain` / `dehaze` / `chromatic`，默认 0；旧文档缺字段按 0 读取。降噪、去雾与色差在颜色管线前段执行，暗角与颗粒在颜色/锐化之后执行，任一非零都回退 CPU。
9. 裁剪角度（schema 11）：`CropRect.angle` 为 -45..45 的顺时针角度；渲染先把**完整画面**绕中心旋转并扩展到包围盒（四角透明），`x/y/width/height` 再作用于旋转后的坐标系。`angle = 0` 时与旧文档逐像素一致，缺字段按 0 读取；`is_full()` 在角度非零时恒为 false。前端裁剪框限制在旋转后画面的最大安全内接矩形内，并限制输出不超过原图宽高：角度变化时选框按物理尺寸保留并自动内缩到新安全区，拖动/缩放也不能越界，因此新裁剪不会产生透明角。后端继续接受文档中的既有矩形，旧数据不被自动改写。
10. 局部修复图层（schema 10）：`LayerKind::Retouch` 保存一组圆形笔触（归一化中心、相对短边半径、可选取样点、硬度、不透明度），渲染时按顺序作用于下方合成结果；修复模式镜像取样并对齐环形均值颜色，克隆模式按取样点偏移复制。上限 400 笔，所有字段经 `sanitized()` 夹紧，旧文档缺少该字段时按无图层读取。
11. 蒙版区域（schema 9）：`LayerMask.regions` 保存一组 `MaskRegion`（归一化中心、半径、旋转角、羽化、强度），用于把五官分析检出的椭圆固化为真实蒙版。坐标一律相对原始源图归一化，因此裁剪变化后仍映射正确；`sanitize()` 上限 32 个区域并把所有字段夹回有限范围，旧文档缺少该字段时按空列表读取。
12. 撤销恢复文档内容时重新分配 `revision`，保证版本号单调，避免前端把旧预览误认为新结果。
13. 历史栈当前不跨重启持久化；参数、图层与开关状态必须跨重启保持。若未来需要完整历史恢复，需新增检查点与迁移。
14. 撤销/重做栈各保留最近 60 步（`MAX_HISTORY`），并共享 **32MB** 的估算内存预算（`MAX_HISTORY_BYTES`）。每条记录都是整份 `ProjectDocument`，而蒙版允许 500 笔 × 4096 点——实测这样一份文档序列化后约 6 MB、单次提交落盘 44 ms，只靠 60 步计数仍可能吃掉数百 MB。预算按结构估算（固定结构 + `layers`/蒙版笔画/修复点/区域/字符串的容量），不做 JSON 序列化；超出时先丢最旧的撤销条目，再丢最旧的重做条目，最近的操作优先保留。若单个快照本身超过 32MB，该次编辑不会留下历史记录，而不是突破内存上限。
15. 快照的创建/切换避免多余整文档克隆：`commit` 把旧 baseline 移入历史、再为新的 baseline 克隆当前文档；`undo`/`redo` 把当前文档移动进另一侧栈，而不是先 `clone()` 当前文档。历史仍不跨重启。
16. 照片集字段（schema 13）：`collectionId` / `collectionName` 存在每张照片的文档与数据库行上；新建照片时由导入请求决定加入现有集合还是新建集合。旧文档缺少字段时 `normalized()` 回填为项目自身的 id/name，因此升级后每张旧照片各自成为一个照片集。数据库 schema v2 同步给 `projects` 表增加两列，并在打开旧库时执行 `ALTER TABLE` + 回填。照片集没有独立表，分组由 `list_collections` 派生，空照片集不存在。
17. 逐脸美颜（schema 14）：`PortraitParams.faceIndex` 可选，记录该人像图层对应的检测人脸索引；`None` 表示旧版全局人像层。人脸索引按画面从左到右排序，上限 31。`add_layer` 请求可同时携带 `name` / `portrait` / `mask`，用于一次历史记录创建带脸部椭圆蒙版的独立图层。旧文档缺少字段时按 `None` 读取；`faceIndex` 只承担编辑归属，实际作用范围由蒙版决定，渲染端无需理解人脸索引。
18. 精细修复几何（schema 15）：`PortraitParams.geometry` 保存 `face` / `leftEye` / `rightEye` / `leftBrow` / `rightBrow` / `mouth` / `teeth` 七个可选椭圆，坐标相对原始源图归一化，随图层持久化，因此预览、导出与重开项目都不需要重新检测。旧文档缺少字段时按 `None` 读取，精细修复退化为斑点修复的肤色范围；打开效果时前端会补测一次并把几何写回图层。所有椭圆经 `sanitized()` 夹紧，中心允许 -0.5..1.5，半径 0.0005..2，羽化 0..1。
19. 美颜与塑形（schema 16）：`PortraitParams` 增加 `geometry.neck` / `lips`、`whiten` / `whitenWarmth` / `rosy` / `eyeBag`、七个分区细纹强度、`bodyMode` 与六个身体美颜参数、`faceSlim` / `eyeSize` / `noseSlim`，以及像素蛋糕式细分参数 `smoothRadius` / `whitenSaturation` / `oilControl` / `eyeWhite` / `irisDetail` / `teethBrighten` / `lipTint` / `lipWarmth` / `noseContour` / `eyeshadow` / `browTint` / `jawWidth` / `chinLength` / `eyeDistance`；全部带默认值，旧文档按 0/false 读取。`RetouchParams.warp` 保存液化笔触（归一化点列、半径、强度、`push|pinch|bulge`），上限 200 笔 × 1024 点；`RetouchParams.freeze` 保存冻结保护笔触，复用 `MaskStroke` 的夹紧规则。两者均经 `sanitized()` 处理。`bodyMode` 只改变效果使用的肤色范围与面部排除，不引入新的图层种类。

20. 身体塑形（schema 17/18）：`PortraitParams` 增加 `bodySlim` 与 `bodyLengthen`（0..100，默认 0）。不引入新的图层类型，作为人像图层的形状参数保存；渲染时只在图层蒙版覆盖范围内，向蒙版加权重心水平收窄、对重心以下做垂直拉伸（腿部），蒙版边缘权重平滑归零，并用人脸几何椭圆保护面部。没有蒙版内容时不生效，UI 提示先绘制修饰范围；水平压缩上限 42%、垂直拉伸上限 35%，不会静默改变身体比例。
21. 网格冻结保护（schema 18）：`RetouchParams.freezeGrid` 保存 16×16 行优先布尔网格，覆盖未裁剪的完整画面，因此裁剪与扶正不会改变受保护区域；渲染时把受保护格子栅格化为覆盖率 1，与冻结画笔叠加，液化位移乘以 `1 - coverage`。最多 256 格，`sanitized()` 截断多余项。

## 后果

- 优点：保存/重开一致、撤销成本低、预览缓存可用 revision 直接判新旧。
- 代价：历史在重开项目后从空开始；这是首期明确的限制。
- AI 结果只能作为新图层加入，不能覆盖 base。

## 验证

- 保存/重开后图层参数、顺序、开关一致。
- 一次拖动只产生一条撤销记录；撤销/重做 revision 单调。
- base 图层删除/重排被拒绝。
