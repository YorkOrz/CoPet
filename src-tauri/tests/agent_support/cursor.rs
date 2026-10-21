use super::helpers::{
    home_path, hook_shell, manager_with_fake_agent_names, manager_with_fake_agents, read_json,
    run_hook_shell, HELPER_NAME,
};
use serde_json::json;
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    process::Stdio,
    sync::mpsc,
    time::{Duration, Instant},
};

const EVENTS: &[(&str, &str)] = &[
    ("beforeSubmitPrompt", "user.prompt"),
    ("preToolUse", "tool.before"),
    ("postToolUse", "tool.after"),
    ("postToolUseFailure", "session.error"),
    ("stop", "session.stop"),
    ("sessionEnd", "session.stop"),
];

#[test]
fn cursor_install_merges_user_hooks() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let root = temp.path().join(".copet");
    let hooks_path = home_path(&home, ".cursor/hooks.json");
    fs::create_dir_all(hooks_path.parent().unwrap()).unwrap();
    fs::write(
        &hooks_path,
        serde_json::to_vec_pretty(&json!({
            "version": 1,
            "hooks": {
                "preToolUse": [{ "command": "./user-hook.sh", "timeout": 5 }]
            },
            "userSetting": true
        }))
        .unwrap(),
    )
    .unwrap();
    let manager = manager_with_fake_agents(&root, &home);

    let result = manager.install("cursor").unwrap();
    let hooks = read_json(&hooks_path);

    assert!(result.adapter.installed);
    assert_eq!(result.adapter.id, "cursor");
    assert_eq!(result.adapter.display_name, "Cursor");
    assert_eq!(hooks["version"], 1);
    assert_eq!(hooks["userSetting"], true);
    assert!(hooks["hooks"]["preToolUse"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["command"] == "./user-hook.sh"));
    for (event, kind) in EVENTS {
        let entries = hooks["hooks"][event].as_array().unwrap();
        assert!(
            entries.iter().any(|entry| {
                entry["command"].as_str().is_some_and(|command| {
                    command.contains(HELPER_NAME) && command.contains(&format!(" cursor {kind}"))
                })
            }),
            "{event} missing CoPet hook"
        );
    }
}

#[test]
fn cursor_install_is_idempotent_and_repair_keeps_one_copet_entry_per_event() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let root = temp.path().join(".copet");
    let manager = manager_with_fake_agents(&root, &home);

    manager.install("cursor").unwrap();
    manager.install("cursor").unwrap();
    manager.repair("cursor").unwrap();

    let hooks = read_json(home_path(&home, ".cursor/hooks.json"));
    for (event, kind) in EVENTS {
        let count = hooks["hooks"][event]
            .as_array()
            .unwrap()
            .iter()
            .filter(|entry| {
                entry["command"].as_str().is_some_and(|command| {
                    command.contains(HELPER_NAME) && command.contains(&format!(" cursor {kind}"))
                })
            })
            .count();
        assert_eq!(count, 1, "{event} should have exactly one CoPet hook");
    }
}

#[test]
fn cursor_install_accepts_cursor_agent_executable() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let root = temp.path().join(".copet");
    let manager = manager_with_fake_agent_names(&root, &home, &["cursor-agent"]);

    let result = manager.install("cursor").unwrap();

    assert!(result.adapter.installed);
    assert!(home_path(&home, ".cursor/hooks.json").exists());
}

#[test]
fn cursor_install_detection_handles_shell_significant_helper_paths() {
    assert_cursor_install_detection_for_root_dir_name("semi;quote'root");
}

#[test]
fn cursor_install_detection_handles_then_delimiter_in_helper_path() {
    assert_cursor_install_detection_for_root_dir_name("semi; then root");
}

#[test]
fn cursor_install_detection_handles_else_delimiter_in_helper_path() {
    assert_cursor_install_detection_for_root_dir_name("semi; else root");
}

fn assert_cursor_install_detection_for_root_dir_name(root_dir_name: &str) {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let root = temp.path().join(root_dir_name);
    let manager = manager_with_fake_agents(&root, &home);

    let installed = manager.install("cursor").unwrap();
    assert!(installed.adapter.installed);

    let inspected = manager.inspect("cursor").unwrap();
    assert!(inspected.installed);

    let uninstalled = manager.uninstall("cursor").unwrap();
    assert!(!uninstalled.adapter.installed);

    let hooks = read_json(home_path(&home, ".cursor/hooks.json"));
    assert!(!hooks.to_string().contains("copet-hook.sh"));
}

#[test]
fn cursor_stale_helper_path_is_not_installed_and_install_repairs_it() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let stale_root = temp.path().join("stale-copet-root");
    let current_root = temp.path().join("current-copet-root");
    let stale_manager = manager_with_fake_agents(&stale_root, &home);
    let current_manager = manager_with_fake_agents(&current_root, &home);

    stale_manager.install("cursor").unwrap();

    let stale_summary = current_manager.inspect("cursor").unwrap();
    assert!(!stale_summary.installed);

    let repaired = current_manager.install("cursor").unwrap();
    assert!(repaired.adapter.installed);

    let hooks = read_json(home_path(&home, ".cursor/hooks.json"));
    let serialized = hooks.to_string();
    // 序列化后的 JSON 会把 Windows 路径里的反斜杠再转义一层。
    let expected = serde_json::to_string(
        &current_root
            .join("hooks")
            .join(HELPER_NAME)
            .to_string_lossy()
            .to_string(),
    )
    .unwrap();
    let stale = serde_json::to_string(
        &stale_root
            .join("hooks")
            .join(HELPER_NAME)
            .to_string_lossy()
            .to_string(),
    )
    .unwrap();
    assert!(serialized.contains(expected.trim_matches('"')));
    assert!(!serialized.contains(stale.trim_matches('"')));
}

#[test]
fn cursor_uninstall_removes_only_copet_hooks() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let root = temp.path().join(".copet");
    let hooks_path = home_path(&home, ".cursor/hooks.json");
    fs::create_dir_all(hooks_path.parent().unwrap()).unwrap();
    fs::write(
        &hooks_path,
        serde_json::to_vec_pretty(&json!({
            "version": 1,
            "hooks": {
                "preToolUse": [{ "command": "./user-hook.sh" }]
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let manager = manager_with_fake_agents(&root, &home);

    manager.install("cursor").unwrap();
    let result = manager.uninstall("cursor").unwrap();
    let hooks = read_json(&hooks_path);

    assert!(!result.adapter.installed);
    assert!(hooks["hooks"]["preToolUse"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["command"] == "./user-hook.sh"));
    let serialized = hooks.to_string();
    assert!(!serialized.contains(HELPER_NAME));
    assert!(!root.join("adapters/cursor.json").exists());
}

#[test]
fn cursor_uninstall_preserves_forged_mentions_and_unmanaged_events() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let root = temp.path().join(".copet");
    let hooks_path = home_path(&home, ".cursor/hooks.json");
    fs::create_dir_all(hooks_path.parent().unwrap()).unwrap();
    fs::write(
        &hooks_path,
        serde_json::to_vec_pretty(&json!({
            "version": 1,
            "hooks": {
                "preToolUse": [
                    { "command": "echo copet-hook.sh cursor tool.before" }
                ],
                "unmanagedEvent": [
                    { "command": "if [ -f '/tmp/copet-hook.sh' ]; then '/tmp/copet-hook.sh' cursor tool.before; else echo '{}'; fi" }
                ]
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let manager = manager_with_fake_agents(&root, &home);

    let before = manager.inspect("cursor").unwrap();
    assert!(!before.installed);

    manager.install("cursor").unwrap();
    manager.uninstall("cursor").unwrap();
    let hooks = read_json(&hooks_path);

    assert_eq!(
        hooks["hooks"]["preToolUse"][0]["command"],
        "echo copet-hook.sh cursor tool.before"
    );
    assert_eq!(
        hooks["hooks"]["unmanagedEvent"][0]["command"],
        "if [ -f '/tmp/copet-hook.sh' ]; then '/tmp/copet-hook.sh' cursor tool.before; else echo '{}'; fi"
    );
}

#[test]
fn cursor_install_rejects_non_object_hooks_without_panicking() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let root = temp.path().join(".copet");
    let hooks_path = home_path(&home, ".cursor/hooks.json");
    fs::create_dir_all(hooks_path.parent().unwrap()).unwrap();
    fs::write(
        &hooks_path,
        serde_json::to_vec_pretty(&json!({
            "version": 1,
            "hooks": []
        }))
        .unwrap(),
    )
    .unwrap();
    let manager = manager_with_fake_agents(&root, &home);

    let error = manager.install("cursor").unwrap_err().to_string();

    assert!(error.contains("invalid JSON"));
}

#[test]
fn cursor_install_rejects_non_array_event_hooks_without_panicking() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let root = temp.path().join(".copet");
    let hooks_path = home_path(&home, ".cursor/hooks.json");
    fs::create_dir_all(hooks_path.parent().unwrap()).unwrap();
    fs::write(
        &hooks_path,
        serde_json::to_vec_pretty(&json!({
            "version": 1,
            "hooks": {
                "preToolUse": { "command": "./user-hook.sh" }
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let manager = manager_with_fake_agents(&root, &home);

    let error = manager.install("cursor").unwrap_err().to_string();

    assert!(error.contains("invalid JSON"));
}

#[test]
fn cursor_before_submit_prompt_outputs_continue_when_helper_is_missing() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let root = temp.path().join(".copet");
    let manager = manager_with_fake_agents(&root, &home);

    manager.install("cursor").unwrap();
    fs::remove_file(root.join("hooks").join(HELPER_NAME)).unwrap();

    let hooks = read_json(home_path(&home, ".cursor/hooks.json"));
    let command = copet_command(&hooks, "beforeSubmitPrompt");
    let output = run_hook_shell(command);

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "{\"continue\":true}"
    );
}

#[test]
fn cursor_pre_tool_command_posts_tool_input_summary() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let root = temp.path().join(".copet");
    let runtime = temp.path().join("runtime");
    let manager = manager_with_fake_agents(&root, &home);

    manager.install("cursor").unwrap();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!(
        "http://127.0.0.1:{}/v1/events",
        listener.local_addr().unwrap().port()
    );
    fs::create_dir_all(&runtime).unwrap();
    fs::write(runtime.join("event-endpoint"), &endpoint).unwrap();
    fs::write(runtime.join("event-token"), "secret").unwrap();

    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match listener.accept() {
                Ok((mut stream, _addr)) => {
                    stream.set_nonblocking(false).unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut buffer = [0_u8; 4096];
                    let size = stream.read(&mut buffer).unwrap();
                    let request = String::from_utf8_lossy(&buffer[..size]).to_string();
                    let _ =
                        stream.write_all(b"HTTP/1.1 202 Accepted\r\nContent-Length: 2\r\n\r\n{}");
                    sender.send(Some(request)).unwrap();
                    return;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        sender.send(None).unwrap();
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(_) => {
                    sender.send(None).unwrap();
                    return;
                }
            }
        }
    });

    let hooks = read_json(home_path(&home, ".cursor/hooks.json"));
    let command = copet_command(&hooks, "preToolUse");
    let mut child = hook_shell(command)
        .env("COPET_RUNTIME_DIR", &runtime)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(
            br#"{"conversation_id":"c1","hook_event_name":"preToolUse","cwd":"/repo","tool_name":"Shell","tool_input":{"command":"pnpm test:rust"}}"#,
        )
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "{}");

    let request = receiver
        .recv_timeout(Duration::from_secs(11))
        .unwrap()
        .expect("runtime server should receive the Cursor hook event");
    assert!(request.contains("POST /v1/events"));
    assert!(request.contains("Authorization: Bearer secret"));
    assert!(request.contains(r#""agent":"cursor""#));
    assert!(request.contains(r#""kind":"tool.before""#));
    assert!(request.contains(r#""tool":"Shell""#));
    assert!(request.contains(r#""toolInput":{"command":"pnpm test:rust"}"#));
}

fn copet_command<'a>(hooks: &'a serde_json::Value, event: &str) -> &'a str {
    hooks["hooks"][event]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|entry| {
            entry["command"]
                .as_str()
                .filter(|command| command.contains(HELPER_NAME))
        })
        .unwrap()
}
