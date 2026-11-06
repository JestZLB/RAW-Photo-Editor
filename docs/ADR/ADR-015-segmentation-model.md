# ADR-015：用户自备 ONNX 分割模型

- 状态：已接受（推理链路已用合成 ONNX 探针验证；**真实分割权重的质量 / 速度仍未验证**）
- 日期：2026-11-06
- 关联：ADR-014 栅格蒙版、ADR-010 本地 ONNX 降噪

## 背景

栅格蒙版的数据通路已就绪（ADR-014），但还没有任何来源能产生逐像素覆盖。
可选路线有两条：

1. 内置一个分割/抠图权重（常见 1–25 MB），开箱即用；
2. 只实现适配器，由用户提供 `.onnx`。

项目已有先例：ONNX 降噪通道就是「用户自备模型 + 能力预检 + 失败回退」，
而 Photo-HQ 权重因为上游未声明许可被排除在仓库外（ADR-011）。分割权重的
许可证差异更大（有的仅限研究用途），在未确认授权前不应塞进仓库。

## 决策

1. 新增 `crates/segmentation`：`tract` 纯 Rust CPU 推理，输入契约
   `[1,3,512,512]` NCHW float RGB（0..1），输出接受三种布局：
   `[1,1,H,W]`、`[1,2,H,W]`（第二通道为前景）、`[H,W]`。
2. 512 分块、48px 重叠，三角窗羽化后加权合成；短边不足 512 时按边缘钳制补齐。
   分块起点策略为「最后一列/行贴齐边缘」，因此不会越界也不会留缝。
3. 输出激活按**值域**判断：全部落在 0..1 视为概率，否则按 logits 做 sigmoid；
   `NaN`/`Inf` 归一为 0。这样既能兼容 `sigmoid` 已内置的模型，也能兼容输出 logits 的模型。
4. 能力预检使用 512×512 纯中灰实跑一次：模型能加载但输出布局不符时在预检阶段
   就报 `SEGMENTATION_MODEL_INVALID`，而不是在编辑时生成一张废蒙版。结果按
   模型路径缓存，用户换模型即重新预检。
5. 模型来源只有设置里的路径（复用已有的 `.onnx` 文件选择器），不进入文档、
   不进入 SQLite、不进入日志；仓库不存放权重。
6. 结果经 `apply_raster_mask` 写入栅格蒙版（阈值 0.5、软边 0.12 为默认值），
   与画笔/渐变/椭圆按 max 合并。用户可以「移除智能」只删掉分割覆盖，
   保留手绘部分。
7. 浏览器演示明确返回不可用，并给出原因；不伪造结果。

## 后果

- 优点：无需处理权重授权即可交付能力；契约不符会被预检挡住；用户可自由替换模型。
- 代价：**开箱不可用**，必须先有模型；模型质量、速度与内存取决于用户选择。
- **推理链路已验证**：`crates/segmentation/tests/fixtures/make_probe_model.py` 用纯
  CPython 标准库手写 protobuf，生成一个 181 字节的 `ReduceMean` 模型
  （`[1,3,512,512] → [1,1,512,512]`）。它不做分割，但会真实经过 `tract` 的解析、
  优化与执行，因此加载、输入 fact 覆盖、输出布局解析、512 分块 / 重叠羽化合成
  都在集成测试里跑过；app-core 侧还验证了「能力预检 → 推理 → 栅格蒙版落盘 → 渲染」。
- **仍未验证**：真实分割权重在真实照片上的语义质量、耗时与内存。探针的输出只是
  通道均值，不能用来判断主体边缘是否准确、阈值 0.5 / 软边 0.12 是否合适。
  这一项需要拿到符合契约的 `.onnx` 后单独做像素级回归。

## 验证

- `tiles_cover_small_images_once_and_large_ones_with_overlap`：小图单块、700px 两块（188 重叠）、1400px 九块。
- `tile_input_pads_outside_the_image_with_edge_pixels`：越界按边缘钳制，通道布局为 NCHW。
- `single_channel_probabilities_are_used_as_is` / `two_channel_output_takes_the_foreground_channel`
  / `bare_spatial_output_is_accepted` / `unsupported_layouts_are_rejected`：输出契约。
- `logits_are_squashed_but_probabilities_are_not`：值域判断、`NaN` 归一。
- `feather_weights_stay_positive_and_favour_tile_centres`：接缝权重。
- `segmentation_reports_unconfigured_and_broken_models`（app-core）：
  未配置、路径不存在、文件不是模型三条降级路径与稳定错误码。
- `inference.rs`（segmentation）：真实 `tract` 推理；小图单块输出尺寸 / 值域 / 均值，
  700×600 四块拼接无空洞、整体水平不漂移。
- `onnx_probe_drives_the_whole_segmentation_pipeline`（app-core）：探针通过能力预检，
  在虚化图层上生成栅格蒙版并记录 `onnx:foreground` 来源，蒙版 PNG 可解码，
  文档仍能正常渲染预览。
