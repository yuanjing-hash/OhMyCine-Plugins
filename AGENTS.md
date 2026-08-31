# AGENTS.md

This repository owns OhMyCine official plugin source, the public Plugin SDK, the Hub documentation site, the installable Registry, and plugin Release automation.

## Boundaries

- Plugins run only in OhMyCine Server's WASM sandbox. Player never installs or executes plugin code.
- A plugin adapts provider-specific discovery, playback/download plans, and metadata. Upload, transfer, naming, deletion, storage credentials, and local paths remain Server responsibilities.
- PT site adapters remain built into OhMyCine Server and are not distributed through this repository.
- Never weaken Manifest permissions, package validation, same-repository Release URL checks, or Registry version monotonicity for convenience.

## Development

- TypeScript uses the `plugin-sdk/` schemas and versioned Runtime v1 DTOs as the public contract.
- Official Rust/WASM plugins live under `plugins/official/<name>/` and keep Cargo, Manifest, and release metadata versions consistent.
- Hub is a VitePress static site under `hub/`.
- Do not commit `node_modules`, Rust `target`, SDK `dist`, plugin `dist`, or locally built Release assets.

## Release

- Official assets are built only by `.github/workflows/plugin-release.yml` from a `plugin-<name>-v<version>` tag already contained in `main`.
- Never upload a local `.omcp`, Manifest, checksum, or Registry mutation as an official Release.
- Existing public tags, Releases, Registry entries, and package hashes are immutable.
- Use Chinese Conventional Commit descriptions, for example `feat(plugins): 增加新的在线媒体插件`.
