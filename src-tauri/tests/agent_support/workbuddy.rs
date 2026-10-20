use super::helpers::{manager_with_fake_agent_names, manager_with_fake_agents};
use std::fs;

#[test]
fn workbuddy_install_merges_hooks_and_uninstall_preserves_user_config() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let root = temp.path().join(".copet");
    let settings = home.join(".workbuddy/settings.json");
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    fs::write(
        &settings,
        r#"{"sandbox":{"extraAllowWrite":["~/.ssh"]},"hooks":{"Stop":[{"hooks":[{"type":"command","command":"echo user"}]}]}}"#,
    )
    .unwrap();
    let manager = manager_with_fake_agents(&root, &home);

    manager.install("workbuddy").unwrap();
    let installed = fs::read_to_string(&settings).unwrap();
    assert!(installed.contains("extraAllowWrite"));
    assert!(installed.contains("echo user"));
    assert!(installed.contains("copet-hook"));
    assert!(installed.contains("workbuddy"));
    assert!(installed.contains("UserPromptSubmit"));
    assert!(installed.contains("PermissionRequest"));
    assert!(installed.contains("tool.before"));
    assert!(installed.contains("permission.waiting"));
    assert!(root.join("adapters/workbuddy.json").exists());
    assert!(root.join("backups/workbuddy").exists());

    // WorkBuddy 的 HookExecutor 走 PortableGit 的 bash：反斜杠会被当成转义符，
    // 命令必须是正斜杠的 POSIX 形态，否则实测 exit 127。
    let value: serde_json::Value = serde_json::from_str(&installed).unwrap();
    let command = value["hooks"]["PreToolUse"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    assert!(
        !command.contains('\\'),
        "hook command must use forward slashes: {command}"
    );
    assert!(
        command.starts_with("if [ -f "),
        "hook command must use POSIX shell syntax: {command}"
    );
    assert_eq!(value["hooks"]["PreToolUse"][0]["hooks"][0]["timeout"], 5);

    manager.uninstall("workbuddy").unwrap();
    let uninstalled = fs::read_to_string(&settings).unwrap();
    assert!(uninstalled.contains("extraAllowWrite"));
    assert!(uninstalled.contains("echo user"));
    assert!(!uninstalled.contains("copet-hook"));
    assert!(!root.join("adapters/workbuddy.json").exists());
}

#[test]
fn workbuddy_install_creates_settings_file_when_missing() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let root = temp.path().join(".copet");
    let manager = manager_with_fake_agents(&root, &home);

    manager.install("workbuddy").unwrap();

    let settings = home.join(".workbuddy/settings.json");
    assert!(settings.exists());
    let installed = fs::read_to_string(&settings).unwrap();
    assert!(installed.contains("copet-hook"));
    assert!(installed.contains("session.stop"));
}

#[test]
fn workbuddy_is_detected_from_its_config_dir_without_a_cli_on_path() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let root = temp.path().join(".copet");
    fs::create_dir_all(home.join(".workbuddy")).unwrap();
    let manager = manager_with_fake_agent_names(&root, &home, &[]);

    let summary = manager.auto_install_detected_agents();

    assert_eq!(summary.installed, vec!["workbuddy".to_string()]);
    assert!(home.join(".workbuddy/settings.json").exists());
}

#[test]
fn workbuddy_is_skipped_when_neither_cli_nor_config_dir_exists() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let root = temp.path().join(".copet");
    fs::create_dir_all(&home).unwrap();
    let manager = manager_with_fake_agent_names(&root, &home, &[]);

    let summary = manager.auto_install_detected_agents();

    assert!(summary.installed.is_empty());
    assert!(summary.skipped.contains(&"workbuddy".to_string()));
    assert!(!home.join(".workbuddy").exists());
}
