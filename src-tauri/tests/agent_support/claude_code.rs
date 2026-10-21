use super::helpers::{home_path, manager_with_fake_agents, read_json, HELPER_NAME};
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    process::{Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

#[test]
fn claude_install_merges_hooks_and_uninstall_preserves_user_hooks() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let root = temp.path().join(".copet");
    let settings = home_path(&home, ".claude/settings.json");
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    fs::write(
        &settings,
        r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"echo user"}]}]}}"#,
    )
    .unwrap();
    let manager = manager_with_fake_agents(&root, &home);

    manager.install("claude-code").unwrap();
    let installed = fs::read_to_string(&settings).unwrap();
    assert!(installed.contains("echo user"));
    assert!(installed.contains(HELPER_NAME));
    assert!(installed.contains("claude-code"));
    assert!(installed.contains("tool.before"));
    assert!(root.join("adapters/claude-code.json").exists());
    assert!(root.join("backups/claude-code").exists());

    manager.uninstall("claude-code").unwrap();
    let uninstalled = fs::read_to_string(&settings).unwrap();
    assert!(uninstalled.contains("echo user"));
    assert!(!uninstalled.contains(HELPER_NAME));
    assert!(!root.join("adapters/claude-code.json").exists());
}

#[test]
fn claude_install_writes_shell_safe_hook_command() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let root = temp.path().join(".copet");
    let manager = manager_with_fake_agents(&root, &home);

    manager.install("claude-code").unwrap();
    let settings = read_json(home_path(&home, ".claude/settings.json"));
    let command = settings["hooks"]["PreToolUse"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap()
        .to_string();

    assert!(command.contains(HELPER_NAME));
    assert!(command.contains("claude-code tool.before"));
    // Claude Code runs hook commands through Git Bash on every platform,
    // including Windows, where a backslash-separated executable path collapses
    // into an invalid command (exit 127). The command must therefore use POSIX
    // syntax and a forward-slash path.
    assert!(
        command.starts_with("if [ -f "),
        "hook command must use POSIX shell syntax: {command}"
    );
    assert!(
        !command.contains('\\'),
        "hook command must not contain backslashes: {command}"
    );
}

#[test]
fn claude_install_rejects_non_object_hooks_without_panicking() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let root = temp.path().join(".copet");
    let settings = home_path(&home, ".claude/settings.json");
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    fs::write(&settings, r#"{"hooks":"not an object"}"#).unwrap();
    let manager = manager_with_fake_agents(&root, &home);

    let error = manager.install("claude-code").unwrap_err().to_string();

    assert!(error.contains("invalid JSON"));
}

#[test]
fn claude_inspect_ignores_copet_text_outside_hook_commands() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let root = temp.path().join(".copet");
    let settings = home_path(&home, ".claude/settings.json");
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    fs::write(
        &settings,
        r#"{"notes":"copet-hook.sh claude-code tool.before"}"#,
    )
    .unwrap();
    let manager = manager_with_fake_agents(&root, &home);

    let summary = manager.inspect("claude-code").unwrap();

    assert!(!summary.installed);
}

#[test]
fn claude_helper_ignores_cursor_compatibility_hook_payloads() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let root = temp.path().join(".copet");
    let runtime = temp.path().join("runtime");
    let manager = manager_with_fake_agents(&root, &home);

    manager.install("claude-code").unwrap();
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
        let deadline = Instant::now() + Duration::from_millis(700);
        loop {
            match listener.accept() {
                Ok((mut stream, _addr)) => {
                    stream.set_nonblocking(false).unwrap();
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

    let mut child = Command::new(root.join("hooks").join(HELPER_NAME))
        .args(["claude-code", "user.prompt"])
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
            br#"{"conversation_id":"c1","hook_event_name":"beforeSubmitPrompt","cursor_version":"3.6.21","prompt":"explain vuejs"}"#,
        )
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "{}\n");

    let request = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(
        request.is_none(),
        "Cursor compatibility payload should not emit a Claude Code event: {request:?}"
    );
}
