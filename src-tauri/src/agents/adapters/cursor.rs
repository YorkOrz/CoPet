use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::super::{
    hook_command, read_json_object_optional, read_json_object_required, write_json_atomic,
    AdapterError, AgentManager, CliAdapter, HookShell,
};

pub(super) static ADAPTER: CursorAdapter = CursorAdapter;

struct CursorEvent {
    cli_event: &'static str,
    kind: &'static str,
}

const EVENTS: &[CursorEvent] = &[
    CursorEvent {
        cli_event: "beforeSubmitPrompt",
        kind: "user.prompt",
    },
    CursorEvent {
        cli_event: "preToolUse",
        kind: "tool.before",
    },
    CursorEvent {
        cli_event: "postToolUse",
        kind: "tool.after",
    },
    CursorEvent {
        cli_event: "postToolUseFailure",
        kind: "session.error",
    },
    CursorEvent {
        cli_event: "stop",
        kind: "session.stop",
    },
    CursorEvent {
        cli_event: "sessionEnd",
        kind: "session.stop",
    },
];

pub(super) struct CursorAdapter;

impl CliAdapter for CursorAdapter {
    fn id(&self) -> &'static str {
        "cursor"
    }

    fn display_name(&self) -> &'static str {
        "Cursor"
    }

    fn config_path(&self, home: &Path) -> PathBuf {
        home.join(".cursor").join("hooks.json")
    }

    fn is_installed(
        &self,
        manager: &AgentManager,
        config_path: &Path,
    ) -> Result<bool, AdapterError> {
        cursor_config_has_copet_hooks(config_path, self.id(), &manager.helper_path())
    }

    fn install(&self, manager: &AgentManager) -> Result<(), AdapterError> {
        let path = self.config_path(manager.home());
        manager.backup_file(self.id(), &path)?;
        let mut value = read_json_object_optional(&path)?.unwrap_or_else(|| json!({}));
        install_cursor_hooks(&mut value, self.id(), &manager.helper_path(), &path)?;
        write_json_atomic(&path, &value)
    }

    fn uninstall(&self, manager: &AgentManager) -> Result<(), AdapterError> {
        let path = self.config_path(manager.home());
        if !path.exists() {
            return Ok(());
        }

        manager.backup_file(self.id(), &path)?;
        let mut value = read_json_object_required(&path)?;
        remove_cursor_hooks(&mut value, self.id(), &path)?;
        write_json_atomic(&path, &value)
    }

    fn executable_names(&self) -> &'static [&'static str] {
        &["cursor-agent", "cursor"]
    }
}

fn install_cursor_hooks(
    value: &mut Value,
    adapter_id: &str,
    helper_path: &Path,
    path: &Path,
) -> Result<(), AdapterError> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| AdapterError::InvalidJson(path.to_path_buf()))?;
    object.entry("version").or_insert_with(|| json!(1));
    remove_cursor_hooks(value, adapter_id, path)?;

    let object = value
        .as_object_mut()
        .ok_or_else(|| AdapterError::InvalidJson(path.to_path_buf()))?;
    let hooks = object.entry("hooks").or_insert_with(|| json!({}));
    let hooks_object = hooks
        .as_object_mut()
        .ok_or_else(|| AdapterError::InvalidJson(path.to_path_buf()))?;

    for event in EVENTS {
        hooks_object
            .entry(event.cli_event)
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or_else(|| AdapterError::InvalidJson(path.to_path_buf()))?
            .push(json!({
                "command": hook_command(adapter_id, helper_path, event.kind, HookShell::Cmd),
                "timeout": 1
            }));
    }

    Ok(())
}

fn remove_cursor_hooks(
    value: &mut Value,
    adapter_id: &str,
    path: &Path,
) -> Result<(), AdapterError> {
    let Some(hooks) = value.get_mut("hooks") else {
        return Ok(());
    };
    let hooks = hooks
        .as_object_mut()
        .ok_or_else(|| AdapterError::InvalidJson(path.to_path_buf()))?;

    for event in EVENTS {
        let Some(entries) = hooks.get_mut(event.cli_event) else {
            continue;
        };
        let entries = entries
            .as_array_mut()
            .ok_or_else(|| AdapterError::InvalidJson(path.to_path_buf()))?;
        entries.retain(|entry| {
            !entry
                .get("command")
                .and_then(Value::as_str)
                .is_some_and(|command| is_cursor_copet_command(command, adapter_id))
        });
    }

    Ok(())
}

fn cursor_config_has_copet_hooks(
    path: &Path,
    adapter_id: &str,
    expected_helper_path: &Path,
) -> Result<bool, AdapterError> {
    let Some(value) = read_json_object_optional(path)? else {
        return Ok(false);
    };
    let Some(hooks) = value.get("hooks").and_then(Value::as_object) else {
        return Ok(false);
    };

    Ok(EVENTS.iter().all(|event| {
        hooks
            .get(event.cli_event)
            .and_then(Value::as_array)
            .is_some_and(|entries| {
                entries.iter().any(|entry| {
                    entry
                        .get("command")
                        .and_then(Value::as_str)
                        .is_some_and(|command| {
                            is_cursor_copet_kind_command(
                                command,
                                adapter_id,
                                event.kind,
                                expected_helper_path,
                            )
                        })
                })
            })
    }))
}

fn is_cursor_copet_command(command: &str, adapter_id: &str) -> bool {
    cursor_command_matches(command, adapter_id, None, None)
}

fn is_cursor_copet_kind_command(
    command: &str,
    adapter_id: &str,
    kind: &str,
    expected_helper_path: &Path,
) -> bool {
    cursor_command_matches(command, adapter_id, Some(kind), Some(expected_helper_path))
}

/// Cursor 的 hook 命令与其它 adapter 共用 `hook_command`；这里额外要求命令里调用的
/// helper 就是当前安装位置，避免旧路径或别人的调用被当成已安装。
fn cursor_command_matches(
    command: &str,
    adapter_id: &str,
    kind: Option<&str>,
    expected_helper_path: Option<&Path>,
) -> bool {
    let Some(helper_path) = super::super::copet_helper_invocation(command, adapter_id, kind) else {
        return false;
    };
    match expected_helper_path {
        Some(expected) => Path::new(&helper_path) == expected,
        None => true,
    }
}
