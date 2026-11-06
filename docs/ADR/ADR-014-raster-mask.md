# ADR-014：栅格蒙版（分割蒙版的数据通路）

- 状态：已接受（数据与渲染已就绪；模型适配器与界面入口未接入）
- 日期：2026-11-06
- 关联：ADR-009 五官区域分析、ADR-013 命名快照

## 背景

现有蒙版全部是矢量来源：画笔笔触、五官椭圆、线性/径向渐变、启发式肤色范围。它们都能用几十个浮点数表达，随文档 JSON 与 IPC 传输。

分割模型输出的是**逐像素置信度**，这类数据不能按矢量处理：

- 一张 512×512 的 8 位蒙版是 262 KB（PNG 后更小），但 1024×1024 的 f32 数组是 4 MB；
- 它必须能参与现有渲染（与画笔/渐变叠加、羽化、反转、浓度）与导出；
- 它不能塞进 `DocumentState` 的高频响应，否则每次滑块都要搬运几百 KB。

## 决策

1. `LayerMask` 增加 `raster: Option<MaskRaster>`，只存引用与采样参数：
   `asset_id`、`threshold`（0..1）、`softness`（0..0.499）、`source`（诊断标签，截断 48 字符）。
   旧文档没有该字段，`serde(default)` + `skip_serializing_if` 保证前后兼容（有单测锁定）。
2. 像素以项目资产形式保存：`assets/mask-{assetId}.png`，8 位灰度复制进 RGB 三通道，`assets` 表 `kind = "mask"`。
   文档与 IPC 只传 `asset_id`。
3. 渲染层通过 `AssetResolver::load_raster_mask` 获取解码后的 `RasterCoverage`（f32，0..1，行优先）。
   该方法有默认实现（返回 `None`），所以测试解析器与未支持蒙版的调用方无需改动。
4. 采样使用**双线性**、坐标按未裁剪的源图归一化（与笔触一致，裁剪后仍对齐）。
   覆盖率与矢量来源按 **max** 合并，然后才应用全局羽化、反转与浓度——与五官椭圆的合并顺序一致。
5. 解码后的覆盖按 asset id 缓存在服务里，上限 6 条；1024px 蒙版约 4 MB f32，逐帧重解码会拖垮预览。
6. `set_layer_mask` 收到不含 `raster` 的蒙版时**保留**已有栅格覆盖：画笔/渐变界面不会发送栅格数据，
   若直接覆盖，用户下一次涂抹就会静默丢掉分割结果。清空整个蒙版（`mask: null`）仍会连同栅格一起移除。
7. 本轮不新增 IPC 命令与界面入口：分割模型未定之前，先让数据通路可测、可用。
   模型适配器将通过 `AppService::apply_raster_mask` 写入覆盖，接口签名为 `(projectId, layerId, coverage, source)`。

## 后果

- 优点：分割结果有确定的落点；矢量与像素蒙版可以共存；文档体积与 IPC 负载不受蒙版分辨率影响。
- 代价：多一份资产生命周期管理（删除图层/项目时资产仍在，直到项目删除为止）；栅格蒙版目前没有 UI，属内部能力。
- 未覆盖：分割模型、蒙版编辑（阈值/羽化滑块的 UI）、背景虚化等消费方。

## 验证

- `raster_coverage_samples_bilinearly_and_clamps`（render-engine）：双线性取值与边界钳制。
- `raster_mask_merges_into_coverage_and_respects_threshold`：全覆盖 → 1；无资产 → 0。
- `raster_mask_is_combined_with_brush_strokes_by_maximum`：画笔与栅格按 max 合并。
- `masks_written_before_raster_support_still_load`（document）：旧 JSON 反序列化为 `None`。
- `raster_mask_round_trips_and_clamps_provider_values`：越界阈值/软边被钳制，序列化往返一致。
- `raster_mask_confines_an_adjustment_to_the_selected_half`（app-core）：
  在 +1 EV 的调色图层上挂左半覆盖，渲染后左半保留调整、右半回落，且随后的一次画笔编辑仍保留栅格引用。
