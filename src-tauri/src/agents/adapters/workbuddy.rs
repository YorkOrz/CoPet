use std::path::{Path, PathBuf};

use super::super::{
    install_json_hooks, json_config_has_copet_hook, remove_json_hooks, AdapterError, AgentManager,
    CliAdapter, HookEvent, HookShell,
};

pub(super) static ADAPTER: WorkBuddyAdapter = WorkBuddyAdapter;

/// WorkBuddy 适配器
///
/// WorkBuddy 的用户配置与 Claude Code 一样是带 `hooks` 字段的 `settings.json`：
/// - 修改文件: `~/.workbuddy/settings.json`
/// - 改动内容: 仅在 `hooks` 字段中插入 CoPet 自己的条目，沙箱、审批等其它配置原样保留。
/// - 事件名: `UserPromptSubmit`、`PreToolUse`、`PostToolUse`、`PermissionRequest`、`Notification`、`Stop`。
pub(super) struct WorkBuddyAdapter;

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

impl CliAdapter for WorkBuddyAdapter {
    fn id(&self) -> &'static str {
        "workbuddy"
    }

    fn display_name(&self) -> &'static str {
        "WorkBuddy"
    }

    fn config_path(&self, home: &Path) -> PathBuf {
        home.join(".workbuddy").join("settings.json")
    }

    fn is_installed(
        &self,
        _manager: &AgentManager,
        config_path: &Path,
    ) -> Result<bool, AdapterError> {
        json_config_has_copet_hook(config_path, self.id())
    }

    fn install(&self, manager: &AgentManager) -> Result<(), AdapterError> {
        // WorkBuddy 的 HookExecutor 用 PortableGit 的 `bash -c` 跑命令：
        // 必须给正斜杠路径的 POSIX 语法，反斜杠会被 bash 当转义符吃掉（实测 exit 127）。
        // 超时给 5s：bash 冷启动 + helper 等 stdin EOF 差不多要 1s，1s 会被直接杀掉。
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

    /// WorkBuddy 以桌面端为主，多数安装并不会把 CLI 放进 PATH，
    /// 因此只要配置目录已经存在就认为这台机器上装了 WorkBuddy。
    fn is_present(&self, manager: &AgentManager) -> bool {
        manager.executable_exists("workbuddy")
            || self.config_path(manager.home()).exists()
            || manager.home().join(".workbuddy").is_dir()
    }

    fn executable_names(&self) -> &'static [&'static str] {
        &["workbuddy"]
    }
}
