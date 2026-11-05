# ADR-011：内置 Photo-HQ-W4xEX 人像降噪（Real-ESRGAN ncnn）

- 状态：已接受
- 日期：2026-10-22
- 关联：ADR-001、ADR-003、ADR-004、ADR-010、IPC 契约

## 背景

用户明确希望参考 [Waifu2x-Extension-GUI](https://gitee.com/aaronfeng0711/Waifu2x-Extension-GUI)，使用其中的 **Photo-HQ-W4xEX** 模型做肖像降噪。该模型是项目 v3.115.01 加入的 Real-ESRGAN 4 倍模型，权重为 ncnn 的 `.param` + `.bin`（约 66.8MB），不是 ONNX；网络结构包含 23 个 RRDB 块、约 351 个卷积，层类型为 Convolution / Split / Concat / Eltwise / Interp / BinaryOp / Input。

ONNX 通道（ADR-010）无法直接读取 ncnn；而用纯 Rust 实现 351 个卷积的 CPU 推理，对多兆像素照片会慢到不可用。用户要的是实际可用的模型，而不是把权重格式拒之门外。

## 决策

1. 随 Windows 构建**内置**官方 `realesrgan-ncnn-vulkan` v0.2.0 运行器（MIT，约 6MB）与 Photo-HQ-W4xEX 权重，放在 `apps/desktop/src-tauri/resources/realesrgan/`；通过 Tauri `bundle.resources` 打包。模型来源与授权边界记录在 `models/MODEL-NOTICE.md`。
2. 由 Rust 通过 `std::process::Command` 启动运行器，参数固定为 `-i <input.png> -o <output.png> -n Photo-HQ-W4xEX -m models -s 4 -t 192 -f png`；不使用 shell、不拼接用户命令、不从 UI 接受模型名。`-m` 保持相对路径，因为 Tauri 的资源目录带 `\\?\` 前缀，ncnn 会把它错误地与工作目录拼接。
3. 运行器定位顺序：`LUMA_REALESRGAN_DIR` → `%LOCALAPPDATA%/<bundle-id>/engines/realesrgan` → 打包资源目录 → 开发检出的 `src-tauri/resources/realesrgan`。用户可以用环境变量或自有目录替换引擎，不必改代码。
4. 4 倍模型的输出尺寸很大，因此送模输入限制为最长边 **1600px**：先按需缩小，推理得到 4 倍图，再用 Lanczos 缩回原始源图尺寸，最后按强度与原图逐像素混合，保留源 alpha。这样即使是 6000×4000 照片也不会尝试生成 24000px 的中间图。
5. 结果复用 `ai_result` 资产与 AI 结果图层，名称为“AI 降噪 · Photo-HQ”，提交一条撤销记录；原图与已有图层不被修改。PNG 资产与 1600px 预览的保存逻辑与 ONNX 通道共用。
6. 引擎的标准输出/错误重定向到临时日志，不经过管道（避免长时间推理时管道写满阻塞）；主进程每 250ms 轮询子进程，600 秒超时后强制结束。失败时把日志尾部附在内部错误信息里，用户看到的是可操作的提示（Vulkan/驱动/引擎文件）。
7. 引擎默认工作在 GPU（Vulkan）。没有 Vulkan 时明确失败，不退化为数分钟的 CPU 推理；这与“本地渲染加速”的定位一致。
8. UI 将 Photo-HQ 设为默认引擎，强度默认 80%；自选 ONNX 保留为第二选项。强度 0 严格不改变像素，100% 时完全采用模型输出。浏览器演示模式禁用两个本地引擎。
9. 降噪的交互以“降噪”总开关组织：关闭时不渲染强度与 AI 选项；开启后显示降噪强度、色彩降噪与“AI 降噪”开关。AI 是可选路径，首次启用时调用 `check_ai_denoise_capability` 做一次 32×32 的真实推理预检（模型加载 + Vulkan + 路径处理），结果缓存；预检不可用或正式运行失败时，AI 开关保持关闭并回退普通降噪，同时显示可操作的原因。
10. AI 结果图层的强度用图层不透明度表达：正式推理固定使用内部强度 1.0，前端把“降噪强度”写到不透明度上；关闭 AI 时隐藏图层并恢复普通降噪参数，重新开启优先复用已有图层，避免重复推理。总开关关闭时同时隐藏 AI 图层并把普通降噪归零。

## 后果

- 优点：用户无需另装 Waifu2x-Extension-GUI 或自行转换权重；4 倍模型在 768×966 人像上约 15 秒（Intel 核显），结果确实平滑皮肤与压缩噪声；非破坏、可对比、可撤销。能力预检把“显卡不支持”提前暴露；失败自动回退普通降噪，用户不会被卡在不可用的 AI 选项上。
- 代价：安装包增加约 70MB；需要支持 Vulkan 的 GPU 与驱动；模型对皮肤纹理的平滑很强，80% 左右更自然，100% 会出现“塑料感”；没有进度条与取消按钮，只有“本地推理中…”状态；输入上限 1600px 意味着超大照片的细节来自 4 倍重建后再缩小，而不是原生分辨率推理。
- 授权：运行器为 MIT；Photo-HQ 权重来自 AGPL-3.0 的 Waifu2x-Extension-GUI，但上游没有对权重单独声明许可证。当前仅用于本仓库的本地构建；对外分发安装包前必须取得作者许可，或改为让用户自备权重。`MODEL-NOTICE.md` 与 `THIRD_PARTY_NOTICES.md` 记录了这一限制。
- ncnn 运行器是外部进程而不是 Rust 库：需要处理路径、超时、日志与杀进程；但它换来可接受的速度和与上游模型完全一致的推理结果。

## 验证

- 单元测试：`blend_with_source` 在强度 0/0.5/1 时分别保持原图、线性混合与完全采用模型输出，alpha 始终取源图。
- 集成测试（默认 `--ignored`，需 Vulkan）：导入 64×48 样例，调用 `run_photo_hq_denoise`，约 7 秒后新增 `aiResult` 图层“AI 降噪 · Photo-HQ”、生成预览，重开服务后图层仍在。
- 真机（Windows 11，示例人像 768×966，Photo-HQ 强度 80%）：在真实应用点击后约 16 秒完成，生成“AI 降噪 · Photo-HQ”图层与 1600px 预览，状态栏显示 GPU 渲染；皮肤噪点与细纹被明显平滑，头发与眼睛保持清晰。关闭 AI 后图层隐藏、普通降噪恢复到 80%；降低“降噪强度”到 60% 时图层不透明度同步为 0.6；总开关关闭时两者同时停用。
- 回退验证：把 `LUMA_REALESRGAN_DIR` 指向一个只有运行器、没有模型文件的目录并重启应用；AI 开关点击后保持关闭并禁用，显示“Photo-HQ 引擎不完整…”与“没有可用的 AI 降噪引擎，已保持普通降噪”，主降噪仍为 80%，没有新增图层。
- 验证结束后删除示例项目，只保留用户原有的 `meta-source` 与 `P1023649`。
