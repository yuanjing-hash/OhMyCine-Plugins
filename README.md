# OhMyCine Plugins

OhMyCine 官方插件源码、Plugin SDK、Registry、Hub 文档与 Release 仓库。OhMyCine Server 固定默认分支提交并校验 Registry、Manifest、包摘要、权限和兼容范围后，才允许管理员确认安装插件。

在 Server 的“插件 → 仓库设置”中添加：

```text
https://github.com/yuanjing-hash/OhMyCine-Plugins
```

## 仓库结构

```text
plugins/official/   官方 WASM 插件源码
plugin-sdk/         Manifest、Registry、媒体 DTO、打包和发布工具
hub/                插件市场与开发者文档
docs/               插件生态设计文档
ohmycine-plugin-registry.v1.json  可安装版本 Registry
```

当前官方插件：

- `org.ohmycine.bilibili`：Bilibili 在线媒体库 Beta。

Player 源码位于 [OhMyCine](https://github.com/yuanjing-hash/OhMyCine)，Server 源码位于 [OhMyCine-Server](https://github.com/yuanjing-hash/OhMyCine-Server)。插件只在 Server 的 WASM 沙箱中运行，Player 不安装或执行插件代码。

## 本地验证

```powershell
cd plugin-sdk
npm ci
npm run typecheck
npm run verify
npm run validate:repository:online
npm run build:fixture

cd ..\hub
npm ci
npm run build

cd ..
rustup target add wasm32-unknown-unknown
cargo fmt --manifest-path plugins/official/bilibili/Cargo.toml --all -- --check
cargo clippy --manifest-path plugins/official/bilibili/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path plugins/official/bilibili/Cargo.toml --locked
cargo build --manifest-path plugins/official/bilibili/Cargo.toml --target wasm32-unknown-unknown --release --locked
```

Pull Request 和 `main` push 会在 GitHub Actions 中重复执行 SDK、Schema、契约、Hub、Rust、WASM、确定性打包以及线上 Registry 资产校验。

## 官方发布

官方 Release 只能由 `.github/workflows/plugin-release.yml` 从干净 checkout 构建。本地构建用于开发验证，不能上传成官方发布资产。

1. 同时更新插件 `Cargo.toml`、`plugin.template.json` 中的版本以及 `release.json` 发布说明。
2. 合并到 `main` 并等待 Plugins CI 通过。
3. 在该 `main` 提交创建并推送 `plugin-<name>-v<version>` 标签，例如 `plugin-bilibili-v0.3.6`。
4. GitHub Actions 编译 WASM、生成 Manifest、确定性 `.omcp` 与 SHA-256，并创建或严格复验 Release。
5. Action 从公开 Release 地址重新下载资产，复验身份、包内容与 SHA-256 后，才把新版本提交到 Registry。

同版本不同内容、版本回退、标签与 Manifest 不一致、跨仓库 URL、缺少资产或摘要不一致都会终止发布。Registry 更新按全局并发锁串行执行，避免两个插件发布相互覆盖。

## 安全边界

- 默认不自动安装或自动更新第三方插件；安装和新增权限必须由管理员确认。
- 插件不得读取 Server 全局凭据、本地绝对路径，也不得自行上传、移动或删除媒体。
- 插件包只允许声明式 Manifest、WASM 入口和明确列出的受管静态资源。
- 普通 CI 只有仓库读取权限；只有发布工作流拥有 `contents: write`。
- Registry 和 Release 资产始终固定到本仓库，不接受任意下载地址。

详细协议见 [Plugin SDK](plugin-sdk/README.md) 与 [Hub 开发文档](hub/dev/index.md)。
