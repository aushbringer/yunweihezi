# 贡献指南

感谢你愿意为 OpenTerm 出力。

## 提交前

1. 先搜索 Issue，避免重复
2. 大改动建议先开 Issue 讨论方案，避免白做
3. 保持改动聚焦：一个 PR 只解决一件事

## 开发规范

- 前端：Vue 3 `<script setup lang="ts">`，状态放 Pinia，IPC 统一走 `src/ipc/client.ts`
- 后端：Tauri 命令放 `src-tauri/src/commands/`，业务逻辑放 `src-tauri/src/core/`，命令需在 `lib.rs` 的 `generate_handler!` 注册
- 提交前必须通过：

```bash
npx vue-tsc --noEmit
cargo check --manifest-path src-tauri/Cargo.toml
```

- 提交信息建议：`feat: ...` / `fix: ...` / `docs: ...` / `refactor: ...`

## 新增 Tauri 命令清单

- [ ] `commands/xxx_cmds.rs` 中实现 `#[tauri::command]`
- [ ] `commands/mod.rs` 声明模块
- [ ] `lib.rs` 注册到 `generate_handler!`
- [ ] 如涉及系统能力，更新 `src-tauri/capabilities/default.json`
- [ ] `src/ipc/client.ts` 增加调用封装

## 报告问题

请附：系统与版本、OpenTerm 版本、复现步骤、期望/实际结果、相关日志。
日志开启方式：设置环境变量 `RUST_LOG=openterm=debug` 后启动。
