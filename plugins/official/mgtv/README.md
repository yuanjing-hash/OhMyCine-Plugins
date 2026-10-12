# 芒果 TV 只读点播插件

`org.ohmycine.mgtv`，Beta 版本 `0.1.4`，要求 Server `0.1.1` 的插件合同。Beta 安装包由标准 Release 工作流构建；公开资产校验通过后登记官方 Registry。

目录缓存和本系统播放记录分别自 Server `1.1.75`、Player `1.1.60` 引入；Player `1.1.61` 修正通用 HLS 播放网关。插件 `0.1.4` 修正本次视频响应确认的会员档清晰度筛选，保留 `0.1.3` 的内容分类平铺和编码发现，沿用现有 Server、Player 合同。此前的官方调度副本选择与分集分页修复保持；Server 和 Player 本地下载对已知 Clash Fake-IP 使用验证证书的真实 DNS 回退，仍拒绝私网目的地址。普通分类节点没有海报，进入分类后的作品列表使用官方海报。

管理员使用芒果 TV 手机客户端扫码登录，Server Host 捕获官方桌面登录 Cookie 并加密保存。获授权用户共用该连接；连接启用后使用系统已有在线媒体库入口，插件不创建物理媒体库、资源站点或 Server 入库任务。不提供上传、收藏、购买、用券、账号观看记录写入、直播或资源搜索操作。

## 已实现

- 官方国内点播大分类、名称和排序由 `rider/config/platformChannels/v1` 获取。进入综艺、电视剧、电影、少儿、动漫、纪录片或教育后，直接平铺“全部”和官方内容类型子分类；名称与顺序来自该频道启用的 `kind` 标签，不再显示类型、排序、年份、地区或资费等中间筛选目录。没有启用的内容类型时仍可进入“全部”；内容按 `rider/list/pcweb/v3` 分页，旧筛选链接保持兼容。
- 作品采用 `clip:<clipId>`，分集采用 `video:<videoId>`，版本采用 `mgtv:<clipId>:<videoId>`。正片、预告、加更和花絮保留各自视频身份；分页从明确的第 1 页开始，按视频 ID 去重，并只追加一次重复花絮。综艺使用官方期次标题和内容类型，目录排序号不再映射为伪集数。详情的 `defaultSegmentId` 只在官方默认视频确实属于返回分集时提供，客户端已有选集或历史续播优先。
- 官方 `GetMultiPic → GetQrcodeResult → account/setCookie` 扫码流程。临时 ticket 仅用于一次上游请求，不存入插件私有状态、日志或返回 DTO；已捕获 Cookie 仅由 Host 保存。
- 账号和会员摘要采用已观察到的桌面 `uid/uuid`、`nickname`、`isvip/vipinfo.type` 字段。缺失可验证会员信息时返回未知。会员摘要不作为视频授权证明。
- 播放通过当前官方 `video/streamList` 验证精确集数、版本、清晰度和完整流。必须有 `user.purview=200`，没有官方试看组件和 DRM，选中流 URL 非空、格式受支持且时长与完整视频一致。`needPay=0` 保留免费档规则；`needPay=1` 是会员档标记，额外要求同一次精确视频响应的 `user.login=true` 和 `user.isvip=1`（数值或字符串 `"1"`），缺失、未知或矛盾状态拒绝会员档。菜单、默认选择和手动选择共用这条条件；保存的账号摘要、VIP 布尔值、可见 URL、免费清晰度标记均不能单独放行。
- `streamList` 按官方 `vf` 字段声明既有 H.264/H.265/AV1 编码组，补齐可支持流的发现；默认选择最高可用高度，同高度保留首个可用编码，显式清晰度保持精确身份。没有固定 720P 上限；补齐编码请求不等于真实共享 VIP 的 1080P/4K 已恢复，实际账号响应及 Windows/Android 播放仍须分别验收。
- `media.offline_download_plan` 操作 20 返回 Host 不透明资产引用。在完整播放授权基础上，还要求同一视频的官方 `video/info` 明确设置 `config.download.downloadDisplay=1`。这是下载当前已授权完整流的条件，不声称提供官方离线许可证或支持 DRM。
- 官方字幕由当前桌面播放器调用的 `mobile-stream.api.mgtv.com/v2/video/title` 获取；有可用 SRT 资源时注册 Host 引用，供播放和本地下载使用。字幕接口失败不阻断已授权视频。
- CDN 权限使用明确域名。当前真实免费完整视频观察到 `web-disp{,1,2}.titan.mgtv.com`、`pcvideoks.titan.mgtv.com`、`pcvideomigu.titan.mgtv.com`、`pcvideotx.titan.mgtv.com` 与 `pcvideoctyun.titan.mgtv.com`。未审核 CDN 返回时继续尝试同一清晰度的其他已声明调度副本；全部失败后清楚拒绝，不依据上游响应自动扩大权限。

播放失败使用可选固定 `pluginError.reason`，Server 返回自己的安全提示。诊断只记录固定阶段、错误码、原因及当前流的登录状态、权益码和身份匹配布尔值；不记录 Cookie、ticket、账号 ID、媒体 URL 或上游完整响应。登录失效的受限流提示重新登录；会员摘要不替代当前视频检查。

媒体实际下载、HLS 控制清单解析、安全网关、Player 本地组装和离线包属于通用核心合同。此插件不获取本地路径或持久化短期媒体 URL。HLS 播放依赖 Server 的控制清单 URI 重写；本地下载由 Player 获取 Server 解析后的短期传输计划。

## 证据与限制

2026-10-10 实际请求官方分类、配置、片库、详情、分集、免费完整流、VIP 试看流、调度控制响应和空字幕清单；fixtures 保留响应字段并替换临时签名及账号字段。免费电影《浴火之路》实响应的 `purview=200`，720P/576P/480P 可用，而 1080P `needPay=1`；VIP 剧集的低清流虽有 URL，却 `purview=101` 且有试看组件，必须拒绝。

扫码创建、未扫码状态 `203` 和过期状态 `26` 已实际观察。管理员确认扫码、真实桌面 Cookie 捕获、登录过期后重登、当前会员类型与账号共享、VIP 完整流、带字幕内容及其他 CDN 仍需真实账号联调。账号合同单测是明确标记的合成数据，不是成功登录证据。

官方搜索 API `/pc/search/v1` 已从公开桌面脚本定位，实际匿名请求返回 HTTP 403/401、`forbidden=1/2`；插件保留受限错误。已配置账号时按照官方请求附带当前非敏感 uid，Cookie 仍由 Host 注入。成功结果映射以公开脚本的 `contents[].data.url`、`sourceList[].url` 卡片模板为依据，成功响应及登录搜索仍需真实账号验证。内容子分类使用官方单项 `kind` 筛选，其他筛选维度的旧链接仍可读取，组合筛选 UI 尚未提供。超过 1000 个分集或 64 页的详情明确拒绝，避免无界扫描。

2026-10-12 的七频道公开配置均以 `kind` 表示内容分类，首个 `a1` 标签为官方“全部”。目录使用原有 `catalog:<channel>` 作为唯一“全部”，其余标签沿用 `catalog:<channel>:kind:<tag>`，不会改变具体筛选身份或分页含义。更新插件后，Server 的包范围缓存会隔离旧目录；Player 需重新进入或刷新目录才能看到新结构。目录回归使用已有完整电视剧配置和明确标记的七频道有限合成结构，不代表 Windows/Android 页面已经实机验收。

同日保留的官方桌面播放器代码将 `needPay=1` 映射为会员标签，按 URL 判断流可用性，并允许会员选择该档位；此前无条件拒绝全部 `needPay=1` 流的解释过严。`src/media.rs` 的明确合成会员数据覆盖 1080P/2160P 菜单、默认和精确选择，以及未登录、非会员、未知字段、URL 缺失、试看、DRM、格式、时长和独立下载入口拒绝。它们验证本插件规则，不代表实际账号返回过这些高清流或已完成真实播放。

Windows/Android 的真实播放器和账号联调属于单独验收，Rust fixtures 和 WASM 构建不能代替。接口调研和 fixture 验证未执行任何购买、用券或媒体文件下载。

实际完整免费 HLS 控制清单已限量读取：原响应约 1.01 MiB，758 个分片 URI，包含 ENDLIST，无 KEY/MAP/BYTERANGE/DISCONTINUITY；所有子资源同属 `pcvideomigu.titan.mgtv.com`。保存的控制 fixture 已替换全部查询参数值和私有播放参数。控制清单检查不证明视频分片字节已下载或 Player 已实际播放。

## 验证与本地构建

Rust 测试覆盖真实详情/分集分页和官方默认选择、完整脱敏二维码响应、调度副本回退及固定诊断原因：Base64 正文按 Host 4 MiB 载荷边界处理，不能使用显示字段的 4 KiB 长度限制。Server 的 `TestMangoWASMCurrentHostPublicFixtureSmoke` 通过实际构建 WASM 调用当前 Go Host 的 HTTP、storage 与 clock，覆盖官方导航、完整详情/分集和扫码创建。`TestMangoWASMCurrentHostMediaPlaybackAndOfflineEntitlement` 另覆盖免费完整流的播放及 op20 下载、真实 VIP 试看拒绝和合成禁用下载负例，校验 owner-bound asset.register 与当前 Go DTO。其 HTTP 使用真实公开脱敏目录/权益 fixture 和合成 QR、CDN 调度及空字幕控制响应，不访问提供方、不登录真实账号，也不证明媒体分片播放或下载。

```powershell
cargo fmt --manifest-path plugins/official/mgtv/Cargo.toml --all -- --check
cargo clippy --manifest-path plugins/official/mgtv/Cargo.toml --all-targets --locked -- -D warnings
cargo test --manifest-path plugins/official/mgtv/Cargo.toml --locked
./plugins/official/mgtv/build.ps1
```

本地包在插件的 `dist/`，不进入 Git。正式发布继续使用通用 `plugin-<name>-v<version>` 工作流，无须自建发布通道。
