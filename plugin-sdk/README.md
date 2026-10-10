# OhMyCine Plugin SDK

`@ohmycine/plugin-sdk` 定义 Server 非 PT 站点插件的 Registry、Manifest、媒体 DTO 和运行时契约。插件在 Server 的 WASM 沙箱中运行；Player 不安装插件包，也不执行插件代码。

## 构建可安装包

准备符合 `schema/manifest-v1.schema.json` 的 Manifest 模板和已编译 WASM，然后执行：

```powershell
npm install
npm run pack -- --manifest ..\plugins\official\example\plugin.template.json --wasm .\build\plugin.wasm --out .\dist\example
```

输出是两个独立的 GitHub Release 资产：

- `<plugin-id>-<version>.manifest.json`：包含工具计算后的 `packageSha256`。
- `<plugin-id>-<version>.omcp`：内容确定的 ZIP 包，只包含 WASM 入口和 Manifest 明确声明的受管静态资源。

Manifest 不能放进 `.omcp`，否则 Manifest 中的包摘要会与包含 Manifest 自身的压缩包形成循环。不要手工修改生成后 Manifest 的身份、版本、来源或摘要。

Server 安装时还会为安全解包后的完整目录树计算独立摘要，并在确认、启用、回滚和重启恢复前重新计算。即使 `.omcp` 摘要和内容寻址目录名不变，安装后被修改或增加文件的包也不会执行。

## 媒体库静态封面

在线媒体插件可在 Manifest 顶层声明 `libraryArtwork`，让 Server 专区的媒体库卡片显示插件自己的横向封面：

```json
{
  "libraryArtwork": "assets/library-cover.png"
}
```

该路径必须是包内正斜杠相对路径，且只能指向 PNG、JPEG 或 WebP。打包器会校验文件存在、最大 4 MiB、扩展名与文件 magic bytes 一致，再把它加入 `.omcp`；绝对路径、反斜杠、路径穿越、符号链接、SVG、HTML 和未声明的附加文件都会被拒绝。Server 安装和每次读取时会重新验证相同约束。

Player 不直接读取插件目录，也不会请求插件提供的远端图片地址。Server 只为当前启用且完整性有效的安装生成基于包 SHA-256 的同源只读地址 `/api/v1/assets/plugin-covers/<packageSha256>`；URL 不携带 Player Token、插件凭据或本地安装路径。旧插件不声明 `libraryArtwork` 时继续使用 Player 的默认占位封面。

需要用真实内容动态合成封面时，Manifest 另声明 `library.artwork_candidates`。operation 13 返回最多 9 个 `{id, assetRef}`：`id` 是稳定媒体身份，`assetRef` 必须是插件已通过 Host `asset.register` 注册、且绑定当前连接的 UUID。插件不得返回上游图片 URL、Header、Cookie 或本地路径。Server 会重新校验 asset 所有权与网络权限，限制响应和解码尺寸，选择最多 4 张合成 16:9 JPEG，并只向 Player 返回带短时 HMAC 的同源生成图；包内 `libraryArtwork` 继续作为失败兜底。

## 生成 Registry 条目

仓库根目录发布 `ohmycine-plugin-registry.v1.json`。每个条目的 `manifestUrl` 和 `packageUrl` 必须指向同一 GitHub 仓库的 Release 资产，`packageSha256` 必须与生成 Manifest 和 `.omcp` 完全一致。Server 会先固定 Registry 所在提交 SHA，再受控下载 Release 资产。

模板见 [`../plugins/ohmycine-plugin-registry.v1.template.json`](../plugins/ohmycine-plugin-registry.v1.template.json)。发布前执行：

```powershell
npm run verify
npm run typecheck
npm run validate:repository:online
```

官方插件发布由仓库根目录 `.github/workflows/plugin-release.yml` 执行。工作流从 `plugin-<name>-v<version>` 标签解析插件和版本，要求标签提交已进入 `main`，从干净 checkout 编译 WASM，并在公开 Release 资产重新下载验证成功后更新 Registry。本地 `npm run pack` 仅用于开发和复现，不能直接上传为官方资产。

`fixtures/online-media.v1.json` 是 Go Host 与 TypeScript SDK 共读的安全契约夹具，固定验证 Work → Segment → Version → Variant、声明式动作、DASH 双轨、字幕/弹幕、UUID 资产引用和短时选择令牌。修改这些字段时必须同时通过 SDK verify 与 Server 共读测试。

## 在线媒体库导航

Manifest 默认使用 `navigationMode: "flat"`。需要番剧 → 国家/地区 → 栏目等多层目录时，插件显式声明 `navigationMode: "hierarchical"`，并由 `site.navigation` 返回 `branch` 或最终 `feed` 节点。分支只返回插件内部 `nodeKey`；Server 会把它替换为绑定在线媒体库、深度、祖先链和过期时间的签名 `nodeToken`，Player 后续只能用该 token 请求子节点。插件不能向 Player 暴露提供方 URL、Cookie、游标或私有路由。

Server 当前限制最大 8 层、每层最多 100 个节点，并拒绝循环、重复键、越权 token 与模式不一致。普通本地/115 媒体库继续使用 Server 分类规则提供的标准一级分类，不会为了兼容站点插件而变成任意树。

## 本地生命周期 fixture

```powershell
npm run build:fixture
```

该命令生成一个真实、无 Host import 的最小 WASM 插件和可安装资产，用于 Server 安装、启停、升级、回滚和卸载测试。`dist/` 是本地产物，不提交 Git。

## Runtime v1 最小约束

- 必须导出 `omc_api_version() i32` 并返回 `1`。
- 必须导出线性内存、`omc_alloc(size) i32` 与 `omc_invoke(operation, pointer, length) i64`；可以导出 `omc_start()`。
- 唯一允许的 import 是 `ohmycine.host_call`。WASM 不开放 WASI、Socket、文件系统、环境变量、系统命令或数据库。
- 插件操作使用 [`src/runtime.ts`](src/runtime.ts) 中冻结的 operation code 和 JSON DTO；站点交互、扫码登录、播放进度、PlaybackPlan 与 DownloadPlan 均不得另建私有路由。
- Host HTTP 只允许 Manifest 授权的 HTTPS 域名，并限制 DNS、重定向、超时和载荷。Cookie/Token 只能由凭据引用附加，不能返回 WASM。
- 扫码登录使用两阶段 Cookie 捕获：HTTP 返回一次性 `credentialCaptureRef`，插件确认提供方业务状态成功后才调用 `credentialCommit`。引用短时、单次且绑定插件、运行代次、连接、scope 与 origin。
- 远端播放/下载地址先注册成短时 `urlRef`；普通 DTO、日志和 Player 持久状态不能出现最终 CDN URL 或敏感 Header。
- DASH 应同时检查 `baseUrl` 与 `backupUrl`，优先选择已授权域名上的标准 HTTPS 端口。确需提供方 CDN 端口时，Server 在线资产 Host 只接受内建白名单端口并在注册、读取及每次重定向时重复校验域名和公网 IP；普通 Host HTTP 仍禁止自定义端口。
- Manifest 只能声明 SDK 已知 capability 和精确权限。
- PT 站点适配器属于 Server 内建能力，不通过此 SDK 注册。

## 宿主能力边界

### 原生 Player 本地离线下载（Server 0.1.1）

新增 `media.offline_download_plan` 对应 Runtime v1 operation **20**；既有 1–19 编号不变。插件需要 `download.plan` 的当前授权，并按选中的 `workId / segmentId / versionId / variantId` 重新验证提供方完整播放权限。参考 [`schema/offline-download-plan-v1.schema.json`](schema/offline-download-plan-v1.schema.json) 和 [`fixtures/offline-download-plan.v1.json`](fixtures/offline-download-plan.v1.json)。响应只包含 UUID `urlRef`，声明 `progressive`、`dash` 或 `hls`；不得返回裸 URL、敏感 Header、本地路径或未经明确选择的其他清晰度。受 DRM 保护、仅试看、账号未知和过期登录应返回业务错误。

这项能力用于 Player 本地目录，不创建 Server 下载任务，也不要求 `library_ingest`。Server 将短时控制清单展开成有序轨道、初始化段、字节范围与 AES-128 指令；Player 原生层负责下载和封装。普通在线 DTO 保持同源 opaque 引用。只有经设备 read + download 授权的私有 `offline-plan` 响应可以向原生层返回无提供方凭据的短时 HTTPS CDN 地址；需要提供方敏感 Header 时使用专用同源 `offline-assets` 网关，禁止普通播放资产路由读取此类离线引用。该响应、URL、Header 和密钥不得进入日志、WebView 或持久任务记录。

Server 的 HLS 离线支持有界 VOD：每控制清单最多 2 MiB、50000 行、单行 8192 字节；每计划最多 8192 个下载单元、48 次控制读取、32 把 AES-128 密钥及两层 master。支持相对 URI、独立音轨、`EXT-X-MAP`、显式/隐式 byte range 和显式/default sequence IV。要求 `ENDLIST`，拒绝直播、`SAMPLE-AES`、未知 KEYFORMAT、discontinuity、GAP/PART/SKIP 和存在多组可选视频的 master。stable unit ID 基于轨道、sequence/map 顺序及路径/range 摘要，不包含签名 query；刷新时必须校验轨道拓扑并核验已下载字节。

只读扫码插件可声明 `site.auth`，无需 `site.interaction`。`site.auth.start/poll` 沿用 operation 10/11；旧插件的 `site.interaction` 仍被兼容。成功 `credentialCommit` 返回 `credentialVersion`；新的 `site.auth` 插件在 confirmed poll 顶层回传这个正整数，供 Server 对账号摘要做当前凭据版本 CAS。`account.membership` 可选 `{status:"active"|"inactive"|"unknown",label?,expiresAt?:RFC3339}`。该字段仅用于账号展示，不能替代内容鉴权。摘要带观察时间保存，超过 24 小时、会员过期、明确健康状态变化或凭据重置后显示未知。

插件只适配提供方特有事实：登录、内容发现、播放/下载方案与该插件的元数据。插件不得实现上传、移动、删除、冲突处理、重命名或 NFO/JPG 写入，也不会获得本地绝对路径、115 Cookie 或其他 Storage 凭据。

```text
Plugin DownloadPlan / ProviderMetadata
        ↓
Server DownloadService / MediaTool
        ↓
Server TransferService
        ↓
Local executor / cloud UploadDriver
        ↓
Server NFO/JPG + library reconciliation
```

`media.metadata` 只能由产生当前内容身份的同一插件连接调用。Server 会校验 work/segment 身份并把结果保存为不可变快照；它不会注册成本地扫库、115 扫库、qBittorrent 或其他插件可调用的全局刮削器。普通视频因此可以不依赖 TMDB，但 NFO/JPG 仍由 Server 统一生成。

Manifest 可用 `settingsPage` 声明完整的插件专属配置页。插件决定 tab、section 和白名单字段的位置；`credential-status` 会在声明位置渲染 Host 拥有的登录状态、扫码/重新登录动作与二维码，而 Cookie 捕获、轮询和加密保存仍由 Host 执行。普通字段必须受同一 Manifest 的 `configSchema` 约束。任意 Vue/JavaScript/HTML/CSS、裸路由或 Schema 外字段都不是 SDK 能力。
