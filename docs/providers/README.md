# Provider 接入说明

所有 AI 能力通过 `crates/ai-provider` 的统一契约接入。UI 只展示 Provider 能力声明中允许的功能，不根据厂商品牌硬编码。

## 当前实现

| Provider | 类型 | 网络 | 用途 |
| --- | --- | --- | --- |
| 本地演示 Provider | `mock` | 无 | 开发、契约测试、演示完整任务生命周期 |
| OpenAI 兼容 Provider | `openaiCompatible` | 需要 | `POST {baseUrl}/images/edits` 图像编辑 |

## 配置

1. 在应用内打开「AI」面板，选择或新增 Provider。
2. 填写显示名称、Base URL、模型名。
3. API Key 通过 `set_provider_credential` 写入 Windows Credential Manager；界面只显示是否已保存。
4. 使用「测试连接」验证配置。默认演示 Provider 无需网络。

## 新增 Provider 的准入清单

- 声明真实 `ProviderCapabilities`：尺寸、格式、遮罩、取消、结构化参数。
- 请求与响应必须经过适配器解析与 schema 校验，禁止未校验 JSON 进入编辑图。
- 图片结果写入新资产，通过 `apply_ai_result` 成为独立图层。
- 错误必须映射为稳定错误码，并提供普通用户能理解的中文提示。
- 上传前必须能说明目标服务、上传范围、保留策略与费用；默认不用于训练。
- 自定义 URL 必须通过协议、主机、重定向与私网地址校验。
- 凭据不得出现在 DTO、日志、项目文件或错误栈中。
- 提供至少成功、超时、限流、认证失败、无效响应、取消六类契约测试。

## 上传与隐私

- 只有用户勾选上传确认后才能提交云端任务。
- 本地调色、人像与导出不会上传任何图片。
- 本地演示 Provider 在设备内完成处理。
- 远端不支持取消时，应用会停止等待并忽略结果，但远端任务可能继续执行。
