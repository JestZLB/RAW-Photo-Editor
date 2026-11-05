# 第三方组件与许可证注意事项

本项目代码以 MIT 发布，但以下随应用编译/分发的第三方组件使用不同许可证。发布二进制或安装包前必须由合规负责人复核。

## rawler 0.7.2 — RAW 解码

- 许可证：**LGPL-2.1**
- 用途：CR2/CR3/NEF/ARW/DNG/RAF/RW2/ORF/PEF/SRW 等基础 RAW 解码。
- 风险：Rust 默认静态链接。LGPL-2.1 对静态链接分发有重新链接/目标文件提供义务。正式分发前需要：
  1. 采用允许合规分发的动态链接方案，或提供可重新链接的目标文件/相应源码；
  2. 随包附上 LGPL 文本、版权声明与源码获取方式；
  3. 完成传递依赖与 SBOM 审查。
- 上游：https://github.com/dnglab/dnglab

## CenterFace — 人脸检测模型

- 许可证：**MIT**
- 用途：内置人脸检测（与 YuNet 交叉验证）。
- 随包模型：`crates/portrait-engine/assets/centerface.onnx`（约 7.5 MB）
- 许可证原文：`crates/portrait-engine/assets/LICENSE-centerface.txt`
- 上游：https://github.com/Star-Clouds/CenterFace

## YuNet — 轻量人脸检测模型

- 许可证：**MIT**
- 用途：内置轻量人脸检测（与 CenterFace 交叉验证）。
- 随包模型：`crates/portrait-engine/assets/yunet_2026may.onnx`（约 230 KB）
- 许可证原文：`crates/portrait-engine/assets/LICENSE-yunet.txt`
- 上游：https://github.com/opencv/opencv_zoo/tree/main/models/face_detection_yunet

## tract-onnx — 本地 ONNX 推理

- 许可证：**MIT OR Apache-2.0**
- 用途：AI 降噪、CenterFace/YuNet 人脸检测的纯 Rust CPU 推理。
- 边界：内置 CenterFace / YuNet 权重均为 MIT；用户自备的额外 ONNX 模型授权与适用场景由模型来源决定。
- 上游：https://github.com/sonos/tract

## realesrgan-ncnn-vulkan 0.2.0 — Photo-HQ 本地推理运行器

- 许可证：**MIT**（随包保留原文 `apps/desktop/src-tauri/resources/realesrgan/LICENSE`）
- 用途：在支持 Vulkan 的 GPU 上运行 Real-ESRGAN ncnn 模型。
- 上游：https://github.com/xinntao/Real-ESRGAN-ncnn-vulkan
- 随包文件：`realesrgan-ncnn-vulkan.exe`、`vcomp140*.dll`。

## Photo-HQ-W4xEX — 人像降噪权重

- 来源：Waifu2x-Extension-GUI v3.115.01 的 `realesrgan-ncnn-vulkan/models/`。
- 上游：https://gitee.com/aaronfeng0711/Waifu2x-Extension-GUI
- 用途：4 倍人像降噪/放大，由用户在效果组中显式运行，照片不上传。
- 授权：上游未对权重单独声明许可证。当前仅用于本仓库的本地构建；对外分发安装包前必须取得作者许可，或改为由用户自行提供模型文件。
- 说明文件：`apps/desktop/src-tauri/resources/realesrgan/models/MODEL-NOTICE.md`。

## wgpu — GPU 渲染

- 许可证：MIT OR Apache-2.0
- 用途：WGPU 本地渲染（基础照片、部分调色、裁剪与 AI 结果图层）。
- 上游：https://github.com/gfx-rs/wgpu

## 其他 Rust 依赖

`image`、`tauri`、`reqwest`、`rusqlite`、`rawler` 的传递依赖等均按各自许可证分发。发布前应生成 SBOM 并逐项核对。

## 本项目边界

- 人脸检测模型完全本地运行，图片不会上传。
- RAW 解码结果只写入本地项目缓存，原始 RAW 文件保持只读。
- 本地 GPU AI 生成尚未实现；云端 AI 生成由服务商 GPU 执行。
- 本地 AI 降噪包含两条通道：内置 Photo-HQ-W4xEX（含权重，授权见上）与用户自备的 ONNX 模型；两者都在本机推理，不上传照片。
