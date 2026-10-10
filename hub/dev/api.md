# 插件 SDK 与 Host API

公共契约位于仓库的 `plugin-sdk/`，它是插件作者和 Server 运行时之间唯一稳定边界。官方 Bilibili 插件也必须使用这套 SDK，不能导入 Server `internal/` 包。

## 调用模型

插件通过版本化 JSON ABI 接收标准操作：

- `site.navigation`
- `site.feed`
- `site.search`
- `site.detail`
- `media.playback`
- `media.download_plan`

返回值必须使用 SDK 的 `MediaWork → MediaSegment → MediaVersion → StreamVariant` 层级。清晰度只属于当前媒体版本，不能被当成选集或媒体版本。

`MediaWork.defaultSegmentId` 是可选的提供方默认选择，必须精确匹配同一作品返回的某个 `segments[].id`；缺失或不匹配时不能猜测提供方默认集。客户端已有用户选择或历史续播优先于这个默认值。`MediaSegment.index` 只表示目录排序，不能代替 `episodeNumber`；综艺和彩蛋应保留官方标题，并通过已有 `MediaVersion.edition` 显示内容类型。此扩展不改变 WASM ABI 或操作编号。

## Host 能力

Host API 仅提供受控 HTTP、结构化日志、插件私有存储和受控时间。HTTP 请求只能访问已授权域名，具有超时、重定向、响应体和并发限制；凭据通过句柄附加，不把 Cookie/Token 明文返回插件或 Player。

`PlaybackPlan` 和 `DownloadPlan` 中的 URL 使用短时不透明引用。插件不能选择本地绝对路径、提供任意命令或直接写入媒体库；真实下载由 Server 下载到任务暂存区，DASH 合流后进入现有识别、整理和入库流水线。

## 错误与日志

插件返回稳定错误码，例如 `not-authenticated`、`rate-limited`、`upstream-unavailable`、`timeout` 和 `invalid-response`。Server 对错误和返回 DTO 进行二次校验，不把插件栈、路径、Cookie、Token 或上游签名 URL 发给 Player。

Runtime v1 错误使用 `{pluginError:{code,message,reason?}}`。可选 `reason` 是通用固定枚举：`entitlement-required`、`region-restricted`、`drm-unsupported`、`quality-unavailable`、`incomplete-stream`、`asset-domain-denied`、`network-access-denied`、`download-unavailable`。Server 按当前权限和受支持的枚举返回自己的安全提示；`message` 始终是不可信的诊断文本，不会直接展示给用户。旧插件可继续省略 `reason`，未知值也不会被解释为客户端提示。该字段不改变 WASM ABI 或操作编号。

日志的 `plugin_id`、`connection_id` 和 operation 由宿主绑定，插件不能覆盖，并统一经过 Server 脱敏和轮转。
