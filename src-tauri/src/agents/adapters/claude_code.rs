use std::path::{Path, PathBuf};

use super::super::{
    install_json_hooks, json_config_has_copet_hook, remove_json_hooks, AdapterError, AgentManager,
    CliAdapter, HookEvent, HookShell,
};

pub(super) static ADAPTER: ClaudeCodeAdapter = ClaudeCodeAdapter;

/// Claude Code 适配器
///
/// 该适配器负责与 Claude Code 集成：
/// - 修改文件: `~/.claude/settings.json`
/// - 改动内容: 在 `hooks` 字段中插入 CoPet 特定的 JSON 钩子配置。
///   这些钩子会在 Claude Code 触发特定事件（如 `UserPromptSubmit`, `PreToolUse` 等）时，
///   通过命令行调用 CoPet 的 hook 命令，从而通知桌面宠物更新状态。
pub(super) struct ClaudeCodeAdapter;

const EVENTS: &[HookEvent] = &[
    HookEvent {
        cli_event: "UserPromptSubmit",
        matcher: None,
        kind: "user.prompt",
    },
    HookEvent {
        cli_event: "PreToolUse",
        matcher: Some("*"),
        kind: "tool.before",
    },
    HookEvent {
        cli_event: "PostToolUse",
        matcher: Some("*"),
        kind: "tool.after",
    },
    HookEvent {
        cli_event: "PermissionRequest",
        matcher: Some("*"),
        kind: "permission.waiting",
    },
    HookEvent {
        cli_event: "Notification",
        matcher: None,
        kind: "permission.waiting",
    },
    HookEvent {
        cli_event: "Stop",
        matcher: None,
        kind: "session.stop",
    },
];

impl CliAdapter for ClaudeCodeAdapter {
    fn id(&self) -> &'static str {
        "claude-code"
    }

    fn display_name(&self) -> &'static str {
        "Claude Code"
    }

    fn config_path(&self, home: &Path) -> PathBuf {
        home.join(".claude").join("settings.json")
    }

    fn is_installed(
        &self,
        _manager: &AgentManager,
        config_path: &Path,
    ) -> Result<bool, AdapterError> {
        json_config_has_copet_hook(config_path, self.id())
    }

    fn install(&self, manager: &AgentManager) -> Result<(), AdapterError> {
        // Claude Code 在所有平台上都用 POSIX shell 执行 hook，Windows 上走
        // Git Bash：反斜杠路径会被当成转义符吃掉（实测 exit 127），必须给
        // 正斜杠的 POSIX 语法。超时给 5s：bash 冷启动 + helper 等 stdin EOF
        // 接近 1s，沿用 1s 会被直接杀掉。
        install_json_hooks(
            manager,
            self.id(),
            &self.config_path(manager.home()),
            EVENTS,
            5,
            HookShell::Posix,
        )
    }

    fn uninstall(&self, manager: &AgentManager) -> Result<(), AdapterError> {
        remove_json_hooks(manager, self.id(), &self.config_path(manager.home()))
    }

    fn executable_names(&self) -> &'static [&'static str] {
        &["claude"]
    }
}
