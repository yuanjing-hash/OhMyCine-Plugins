$ErrorActionPreference = 'Stop'
$pluginRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$repositoryRoot = Resolve-Path (Join-Path $pluginRoot '..\..\..')
rustup target add wasm32-unknown-unknown
if ($LASTEXITCODE -ne 0) { throw 'WASM target install failed' }
cargo build --manifest-path (Join-Path $pluginRoot 'Cargo.toml') --target wasm32-unknown-unknown --release --locked
if ($LASTEXITCODE -ne 0) { throw 'MangoTV WASM build failed' }
$wasmPath = Join-Path $pluginRoot 'target\wasm32-unknown-unknown\release\ohmycine_plugin_mgtv.wasm'
node (Join-Path $repositoryRoot 'plugin-sdk\scripts\pack-plugin.mjs') --manifest (Join-Path $pluginRoot 'plugin.template.json') --wasm $wasmPath --out (Join-Path $pluginRoot 'dist')
if ($LASTEXITCODE -ne 0) { throw 'MangoTV package validation failed' }
