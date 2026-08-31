# OhMyCine Hub 与官方插件仓库设计

## 1. 职责

`OhMyCine-Plugins` 同时承载官方插件源码、公开 SDK、静态 Hub、可安装 Registry 和 GitHub Release。Hub 是浏览与开发文档站点，不是插件运行后端；插件只由 OhMyCine Server 安装并在受限 WASM Host 中运行。

```text
官方插件源码 + Plugin SDK
          │
          │ GitHub Actions 干净构建
          ▼
Manifest + .omcp + SHA-256 ──► GitHub Release
          │                         │
          │ 下载复验成功            │ Server 固定提交与 Release
          ▼                         ▼
Registry(main) ───────────────► Server 安装预览/权限确认/WASM 沙箱
          │
          └────────────────────► Hub 展示与开发文档
```

Player 不下载、安装或执行插件。Player 只消费 Server 鉴权并归一化后的在线媒体 DTO、播放方案和同源资产地址。

## 2. 仓库边界

```text
plugins/official/<name>/
  Cargo.toml
  plugin.template.json
  release.json
  src/
plugin-sdk/
  schema/
  src/
  scripts/
hub/
ohmycine-plugin-registry.v1.json
```

- `plugin.template.json` 是 Manifest 单一源码，`packageSha256` 保留占位符，由打包器写入真实摘要。
- `release.json` 只保存频道、分类和发布说明，不保存 URL 或摘要。
- Registry 只描述已经发布且复验成功的版本，不提前指向尚不存在的资产。
- 每条官方 Manifest `source`、Registry 首页及 Release URL 必须固定到 `yuanjing-hash/OhMyCine-Plugins`。

## 3. 插件包

`.omcp` 是确定性 ZIP，只包含：

- Manifest 声明的 WASM 入口；
- 可选、Manifest 明确声明的 PNG/JPEG/WebP 受管静态资源。

Manifest 作为独立 Release 资产，不放入 `.omcp`，避免 Manifest 中包摘要与包含自身的压缩包形成循环。打包器固定条目名、时间戳和压缩参数；CI 使用同一 WASM 连续打包两次并逐字节比较。

## 4. 自动发布

标签格式固定为 `plugin-<name>-v<strict-semver>`。发布工作流必须：

1. 验证标签提交已进入远端 `main`，目录名、Cargo 版本和 Manifest 版本完全一致。
2. 执行 SDK typecheck、Schema/契约校验、Rust fmt/clippy/test 和 WASM Release build。
3. 两次确定性打包并验证 Manifest、WASM、静态资源集合和 SHA-256。
4. 创建 Release；若同标签 Release 已存在，只允许资产逐字节相同的幂等重试。
5. 从公开 Release URL 重新下载 Manifest、`.omcp` 和 SHA-256，重新验证。
6. 仅在验证成功后更新 Registry；拒绝版本回退和同版本内容变化。

普通 CI 使用 `contents: read`。发布工作流独占 Registry 发布并发组，只有它使用 `contents: write`。官方发布不接受本地上传产物。

## 5. 安装与运行安全

- Server 固定 Registry 的 40 位提交 SHA，再读取和校验 Registry。
- Manifest、包和图标必须来自同一仓库 GitHub Release，不能使用任意 raw URL。
- 安装前展示权限，新增权限的升级需要再次确认；默认不自动安装或自动更新。
- WASM 不开放 WASI、Socket、文件系统、环境变量、系统命令或数据库。
- 网络、凭据、私有 KV、日志、下载方案和媒体元数据只能通过版本化 Host API 使用。
- 插件不获得 Storage 凭据、本地绝对路径、全局 Cookie、上传、移动或删除媒体能力。
- 插件设置页是 Manifest 声明式组件树，不接受任意 HTML、JavaScript、Vue 或 CSS。

完整 Schema 和 Runtime v1 约束位于 `plugin-sdk/`，Server 安装生命周期的可执行合同位于 `.trellis/spec/backend/plugin-repository-discovery.md`。

## 6. Hub

Hub 使用 VitePress 静态构建，内容来自已校验 Registry 和仓库文档。它可以提供插件浏览、版本、权限说明、安装仓库地址和开发文档，但不持有 Server 凭据，也不代替 Server 执行安装。
