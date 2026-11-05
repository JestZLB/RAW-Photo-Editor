# ADR-008：导出元数据与位深策略

- 状态：已接受
- 日期：2026-10-21
- 关联：ADR-001、ADR-003、ADR-007、IPC 契约

## 背景

导出会重新生成像素：方向已在解码时烘焙进画面，裁剪与调色也让原图缩略图失效。直接把源 EXIF 复制过去会导致照片被二次旋转、并附带一张能看到"裁掉的内容"的旧缩略图（隐私问题）。同时 8 位导出会把线性管线的精度截断掉，暗部出现色阶断裂。

## 决策

1. 导出提供三档元数据策略：`keep`（默认）、`privacy`、`none`。
2. `keep` 与 `privacy` 都会：把方向标签改写为 1；删除 `JPEGInterchangeFormat` 与长度标签（含 Exif 子 IFD 里的），因为缩略图已过期且可能泄露被裁掉的画面。
3. `privacy` 额外删除 IFD0 的 GPSInfo 指针。默认仍是 `keep`，但界面与导出结果都会明确告知是否仍含位置（`keepsLocation`）。
4. 元数据在导出时从源文件重新读取（只解析头部，不解码像素），不缓存在会话里；源文件永远只读。
5. EXIF 改写只作用于副本，实现是 TIFF/IFD 定点手术：小端与大端都支持，删除条目时重建条目表并前移 next-IFD 指针，块长度不变（多余字节成为不可达空洞，仍是合法 TIFF）。
6. TIFF 的 EXIF 在编码后追写：`image` 没有 EXIF 接口，因此写完后重写 IFD——把源 Exif IFD 与 Make/Model/Software/DateTime/Artist/Copyright 复制到文件尾，重建 IFD0 并回填头部偏移（现有像素与 StripOffsets 不动）；Keep 策略同时复制 GPS IFD。导出后读回文件确认 34665 真的存在，再决定 `metadataWritten`，写失败就如实回报 false。XMP 只读不写。
7. 16 位导出（PNG/TIFF）必须走 CPU 线性管线：GPU 路径输出的是 8 位像素，从它放大到 16 位是假精度。渲染→线性缩放→`linear_to_srgb_u16` 量化。
8. JPEG 请求 16 位在参数校验阶段就返回 `BIT_DEPTH_UNSUPPORTED`，不产生任何文件。
9. 浏览器演示模式无法写 EXIF 与 16 位样本，界面直接禁用这些选项并说明原因。

## 后果

- 优点：不会二次旋转，不泄露过期缩略图，隐私可选且状态可被用户验证；16 位导出是真实的线性精度而非 8 位拉伸。
- 代价：TIFF 的 EXIF 依赖自研 IFD 手术，需持续用真实相机文件回归；XMP 编辑历史仍无法保留。
- 自研的 EXIF 定点修改需要真实相机文件持续回归，当前用合成 IFD（含子 IFD、大小端）覆盖。

## 验证

- 合成 EXIF（方向 6 + GPS 指针 + 子 IFD 缩略图）经服务导出后：方向为 1、无关标签保留、缩略图标签消失、`privacy` 下 GPS 指针移除、`none` 下无 eXIf。
- 真机：同一份带 EXIF 的 JPEG 导入后导出 PNG/JPEG/TIFF，从磁盘解析 `eXIf` 块与 JPEG `APP1` 段确认上述行为；TIFF 的 IFD0 经 34665 指向新 Exif IFD，方向不再重复标注、缩略图已移除、`privacy` 下 GPS 指针消失，`metadataWritten = true`。
- 16 位：PNG IHDR 位深为 16、TIFF BitsPerSample 为 16 且 SampleFormat 为无符号，导出后端为 `cpu`，自定义尺寸同样生效；JPEG 返回 `BIT_DEPTH_UNSUPPORTED` 且目标文件不存在。
