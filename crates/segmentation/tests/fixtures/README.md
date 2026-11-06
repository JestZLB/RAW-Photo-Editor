# 测试探针（不是分割模型）

`channel_mean.onnx`（181 字节）由 `make_probe_model.py` 生成：

```bash
python make_probe_model.py
```

它是一个 `ReduceMean`（对通道维求均值）模型，输入 `[1,3,512,512]`，输出
`[1,1,512,512]`。它**不做分割**，输出等于每像素三通道均值，只用于让 `tract`
真实地解析、优化、运行一个 ONNX 模型，从而验证：

- 模型加载与输入 fact 覆盖（`SegmentationModel::load`）；
- 输出布局解析、`[1,3,512,512]` 分块与重叠羽化合成（`segmentation::segment`）；
- app-core 的能力预检 → 推理 → 栅格蒙版资产落盘 → 渲染。

相关测试：`crates/segmentation/tests/inference.rs`、
`crates/app-core/tests/service.rs::onnx_probe_drives_the_whole_segmentation_pipeline`。

用纯 CPython 标准库手写 protobuf 编码，不依赖 `onnx` 包，也不需要联网；
任何克隆都能重新生成同一个字节文件。真实分割权重的质量、速度与内存占用
仍需在提供 `.onnx` 后单独验证（见 `docs/ADR/ADR-015-segmentation-model.md`）。
