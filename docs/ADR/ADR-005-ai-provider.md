# ADR-005：AI Provider 契约、凭据与上传确认

- 状态：已接受
- 日期：2026-10-20
- 关联：架构文档 08 / 11

## 背景

云端图像编辑是产品能力之一，但不能把 UI 绑定到单一厂商，也不能把未校验的模型响应写入编辑图。图片上传涉及隐私与费用，API Key 不能进入前端持久化、项目文件或日志。

## 决策

1. 统一 Provider 契约：能力声明（输入/输出/取消/幂等）、任务生命周期、稳定错误码与用户消息。
2. 首期提供两种实现：
   - `mock`：完全本地、不上传图片（用于演示与契约测试）；
   - `openaiCompatible`：`/images/edits` 适配器，处理 b64/url 结果、超时、取消与响应大小限制。
3. Provider 原始响应必须先解析、校验并落盘为独立资产，再通过 `apply_ai_result` 创建新图层。禁止覆盖源图。若用户在 Provider 收尾期间取消，任务保持取消状态，刚写入的资产行与文件会被立即删除，而不是留成无人引用的孤儿。
4. API Key 只通过一次 IPC 写入 Windows Credential Manager（`keyring`，service `com.lumaportrait.editor`），数据库仅存非敏感配置。
5. 自定义 Base URL 必须通过 SSRF 防护：默认仅 https；本地 http 必须显式允许；禁止 URL 内嵌凭据；禁止跟随重定向；禁止私有、链路本地与非指定地址的 IP 字面量。
6. 兼容代理地址做端点归一化：用户粘贴 `.../v1/images/edits`、`.../v1/images/generations` 或 `.../v1/models` 时，内部还原为 API 根，再拼出 `/images/edits` 与 `/models`，避免代理地址只对其中一个端点可用。
7. 代理与 DNS：localhost Provider 使用独立 `reqwest::Client`（`no_proxy`）直连，避免公司系统代理把本地服务变成连接错误；远程 Provider 遵循系统 `HTTPS_PROXY`/`ALL_PROXY`。未配置代理时，请求前解析域名并拒绝任何私有/链路本地结果；配置代理时由代理解析（本地 DNS 预检会破坏代理专用网络）。
8. 上传前必须由用户明确确认服务商、图片范围与提示词；本地演示 Provider 明确标注不上传。
9. 服务端不支持取消时，应用停止等待并丢弃过期结果，但提示远端任务可能仍在执行。
10. Provider 调用失败、超时、限流、认证失败与内容拒绝都映射为稳定错误码，不损坏项目。

## 后果

- 优点：可替换、可测试、可审查；敏感凭据不进入前端。
- 代价：首期只提供一个云端适配器；不同厂商的专有能力暂不暴露。
- 已知限制：真实云端调用需要在设置中配置端点与密钥；自动化测试只覆盖 mock 与 URL 校验。

## 验证

- Mock Provider 契约测试覆盖成功、失败、取消与无效参数。
- AI 结果只以新图层出现，撤销后完全移除。
- 凭据不出现在 SQLite、日志或返回 DTO 中。
- SSRF 校验覆盖 localhost、私有地址、非 https 与重定向；`only_real_loopback_hosts_are_local` 确认 `localhost.evil.com` / `127.0.0.1.evil.com` 不会被误判为本地地址。
- URL 归一化：`.../v1/images/edits/` 与 `.../v1/models` 都会还原为 `.../v1`，再正确拼出 `/images/edits` 和 `/models`。
- 前端 Provider 地址识别覆盖 `127.0.0.0/8`、`[::1]` 与大小写 localhost，并拒绝前缀伪装域名。
- 跨项目竞态：切换照片后，尚未完成的 AI 降噪/应用结果/自动校正/白平衡/LUT/区域识别不会写入新项目（由 `isCurrentProject` 守卫）。
