use std::{
    env, fs,
    path::{Path, PathBuf},
};

use serde_json::{json, Value};

use super::super::{
    hook_command, read_json_object_optional, write_json_atomic, AdapterError, AgentManager,
    CliAdapter, HELPER_NAME,
};

pub(super) static ADAPTER: CopilotCliAdapter = CopilotCliAdapter;

struct CopilotEvent {
    cli_event: &'static str,
    kind: &'static str,
}

const EVENTS: &[CopilotEvent] = &[
    CopilotEvent {
        cli_event: "userPromptSubmitted",
        kind: "user.prompt",
    },
    CopilotEvent {
        cli_event: "preToolUse",
        kind: "tool.before",
    },
    CopilotEvent {
        cli_event: "postToolUse",
        kind: "tool.after",
    },
    CopilotEvent {
        cli_event: "permissionRequest",
        kind: "permission.waiting",
    },
    CopilotEvent {
        cli_event: "agentStop",
        kind: "session.stop",
    },
    CopilotEvent {
        cli_event: "errorOccurred",
        kind: "session.error",
    },
];

pub(super) struct CopilotCliAdapter;

impl CliAdapter for CopilotCliAdapter {
    fn id(&self) -> &'static str {
        "copilot"
    }

    fn display_name(&self) -> &'static str {
        "Copilot CLI"
    }

    fn config_path(&self, home: &Path) -> PathBuf {
        copilot_home(home).join("hooks").join("copet.json")
    }

    fn ensure_supported(&self) -> Result<(), AdapterError> {
        #[cfg(windows)]
        {
            return Err(AdapterError::UnsupportedPlatform {
                display_name: self.display_name().to_string(),
                platform: "Windows",
            });
        }

        #[cfg(not(windows))]
        {
            Ok(())
        }
    }

    fn is_supported(&self) -> bool {
        !cfg!(windows)
    }

    fn is_installed(
        &self,
        manager: &AgentManager,
        config_path: &Path,
    ) -> Result<bool, AdapterError> {
        #[cfg(windows)]
        {
            let _ = manager;
            let _ = config_path;
            Ok(false)
        }

        #[cfg(not(windows))]
        {
            copilot_config_has_copet_hooks(config_path, self.id(), &manager.helper_path())
        }
    }

    fn install(&self, manager: &AgentManager) -> Result<(), AdapterError> {
        let path = self.config_path(manager.home());
        let _ = read_json_object_optional(&path)?;
        manager.backup_file(self.id(), &path)?;
        write_json_atomic(
            &path,
            &copilot_hooks_file(self.id(), &manager.helper_path()),
        )
    }

    fn uninstall(&self, manager: &AgentManager) -> Result<(), AdapterError> {
        let path = self.config_path(manager.home());
        if !path.exists() {
            return Ok(());
        }

        manager.backup_file(self.id(), &path)?;
        fs::remove_file(path)?;
        Ok(())
    }

    fn executable_names(&self) -> &'static [&'static str] {
        &["copilot"]
    }
}

fn copilot_home(home: &Path) -> PathBuf {
    env::var_os("COPILOT_HOME")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".copilot"))
}

fn copilot_hooks_file(adapter_id: &str, helper_path: &Path) -> Value {
    let mut hooks = serde_json::Map::new();

    for event in EVENTS {
        hooks.insert(
            event.cli_event.to_string(),
            json!([{
                "type": "command",
                "bash": hook_command(adapter_id, helper_path, event.kind),
                "timeoutSec": 1
            }]),
        );
    }

    json!({
        "version": 1,
        "hooks": hooks
    })
}

fn copilot_config_has_copet_hooks(
    path: &Path,
    adapter_id: &str,
    expected_helper_path: &Path,
) -> Result<bool, AdapterError> {
    let Some(value) = read_json_object_optional(path)? else {
        return Ok(false);
    };
    if value.get("version").and_then(Value::as_u64) != Some(1) {
        return Ok(false);
    }
    let Some(hooks) = value.get("hooks").and_then(Value::as_object) else {
        return Ok(false);
    };

    Ok(EVENTS.iter().all(|event| {
        hooks
            .get(event.cli_event)
            .and_then(Value::as_array)
            .is_some_and(|entries| {
                entries.iter().any(|entry| {
                    hook_entry_matches_event(entry, adapter_id, event.kind, expected_helper_path)
                })
            })
    }))
}

fn hook_entry_matches_event(
    entry: &Value,
    adapter_id: &str,
    kind: &str,
    expected_helper_path: &Path,
) -> bool {
    entry.get("type").and_then(Value::as_str) == Some("command")
        && entry.get("timeoutSec").and_then(Value::as_u64) == Some(1)
        && entry
            .get("bash")
            .and_then(Value::as_str)
            .is_some_and(|command| {
                is_copilot_copet_command(command, adapter_id, kind, expected_helper_path)
            })
}

fn is_copilot_copet_command(
    command: &str,
    adapter_id: &str,
    kind: &str,
    expected_helper_path: &Path,
) -> bool {
    let Some(rest) = command.strip_prefix("if [ -f ") else {
        return false;
    };
    let Some((guard_segment, after_then)) = split_once_unquoted(rest, "; then ") else {
        return false;
    };
    let Some((then_invocation, _)) = split_once_unquoted(after_then, "; else ") else {
        return false;
    };
    let Some(guard_path) = parse_guard_path(guard_segment.trim()) else {
        return false;
    };

    let then_invocation = then_invocation.trim();
    invocation_matches_copet_helper(
        then_invocation,
        adapter_id,
        kind,
        &guard_path,
        expected_helper_path,
    )
}

fn parse_guard_path(segment: &str) -> Option<String> {
    let (path, rest) = parse_shell_quoted_word(segment)?;
    if rest.trim() == "]" {
        Some(path)
    } else {
        None
    }
}

fn invocation_matches_copet_helper(
    invocation: &str,
    adapter_id: &str,
    kind: &str,
    guard_path: &str,
    expected_helper_path: &Path,
) -> bool {
    let Some((helper_path, args)) = parse_shell_quoted_word(invocation) else {
        return false;
    };
    if helper_path != guard_path {
        return false;
    }
    if helper_path != expected_helper_path.to_string_lossy() {
        return false;
    }

    let helper_file_name = Path::new(&helper_path)
        .file_name()
        .and_then(|name| name.to_str());
    if helper_file_name != Some(HELPER_NAME) {
        return false;
    }

    args.trim() == format!("{adapter_id} {kind}")
}

fn split_once_unquoted<'a>(input: &'a str, delimiter: &str) -> Option<(&'a str, &'a str)> {
    let mut in_single_quote = false;
    let mut escaped_outside_quote = false;

    for (index, ch) in input.char_indices() {
        if escaped_outside_quote {
            escaped_outside_quote = false;
            continue;
        }
        if !in_single_quote && ch == '\\' {
            escaped_outside_quote = true;
            continue;
        }
        if ch == '\'' {
            in_single_quote = !in_single_quote;
            continue;
        }
        if !in_single_quote && input[index..].starts_with(delimiter) {
            return Some((&input[..index], &input[index + delimiter.len()..]));
        }
    }

    None
}

fn parse_shell_quoted_word(input: &str) -> Option<(String, &str)> {
    if !input.starts_with('\'') {
        return None;
    }

    let mut value = String::new();
    let mut in_single_quote = false;
    let mut index = 0;
    let mut parsed_any = false;

    while index < input.len() {
        let rest = &input[index..];
        if in_single_quote {
            if rest.starts_with('\'') {
                in_single_quote = false;
                index += 1;
                continue;
            }

            let ch = rest.chars().next()?;
            value.push(ch);
            parsed_any = true;
            index += ch.len_utf8();
            continue;
        }

        if rest.starts_with('\'') {
            in_single_quote = true;
            index += 1;
            continue;
        }
        if rest.starts_with("\\'") {
            value.push('\'');
            parsed_any = true;
            index += 2;
            continue;
        }

        let ch = rest.chars().next()?;
        if ch.is_whitespace() && parsed_any {
            return Some((value, &input[index..]));
        }
        return None;
    }

    if !in_single_quote && parsed_any {
        Some((value, ""))
    } else {
        None
    }
}
