# Research Collaboration Mode

新增 `Research` collaboration mode，wire 值为 `research`，显示名为 `Research`。

该模式用于长时间只读调研。模型应深入收集证据、闭合结论，并避免修改代码、正式文档或配置。中间态可以留在 `docs/todo/` 或记忆中；正式文件只在进入实施阶段并形成完整改动时修改。

运行时入口需要同时覆盖 TUI 和 app-server：

- TUI 新增 `/research` slash command。
- TUI collaboration mode 循环顺序为 `Default -> Research -> Plan -> Default`。
- TUI footer/status 能显示 `Research`。
- app-server `collaborationMode/list` 返回 Research preset。
- app-server `thread/settings/update.collaborationMode` 和 `turn/start.collaborationMode` 接受 `mode: "research"`。

能力边界：

- `request_user_input` 在 Research 模式可用。
- idle extension turn 在 Research 模式被阻止。
- Plan stream parser 只在 Plan 模式启用。
- `update_plan` 在 Research 模式保持可用。

验证覆盖 protocol/preset、TUI `/research` 和循环入口、`request_user_input` 可用性、app-server schema 与本地 standalone 安装。

## Implementation Entry Points

- `ModeKind` 与 `TUI_VISIBLE_COLLABORATION_MODES` 由 `codex-rs/protocol/src/config_types.rs` 持有。
- 内置 presets 由 `codex-rs/models-manager/src/collaboration_mode_presets.rs` 注册。
- collaboration mode prompt 模板由 `codex-rs/collaboration-mode-templates/` 导出。
- TUI slash、循环和状态显示由 `codex-rs/tui/src/collaboration_modes.rs`、`slash_command.rs`、`chatwidget/slash_dispatch.rs`、`chatwidget/settings.rs`、`bottom_pane/footer.rs` 持有。
- app-server list API 和文档由 `codex-rs/app-server-protocol/src/protocol/v2/collaboration_mode.rs`、`common.rs`、`codex-rs/app-server/src/request_processors/catalog_processor.rs`、`codex-rs/app-server/README.md` 持有。
