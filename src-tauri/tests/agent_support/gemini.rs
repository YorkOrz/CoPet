use super::helpers::{home_path, manager_with_fake_agents, read_json, run_hook_shell, HELPER_NAME};
use copet_lib::agents::AgentManager;
use std::fs;

#[test]
fn gemini_install_writes_user_settings_hooks() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let root = temp.path().join(".copet");
    let manager = manager_with_fake_agents(&root, &home);

    let result = manager.install("gemini").unwrap();
    let settings = fs::read_to_string(home_path(&home, ".gemini/settings.json")).unwrap();

    assert!(result.adapter.installed);
    assert!(settings.contains("\"BeforeAgent\""));
    assert!(settings.contains("\"BeforeTool\""));
    assert!(settings.contains("\"AfterTool\""));
    assert!(settings.contains(HELPER_NAME));
    assert!(settings.contains("gemini"));
    assert!(settings.contains("user.prompt"));
    assert!(settings.contains("tool.before"));
}

#[test]
fn gemini_hook_command_exits_successfully_when_helper_is_missing() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let root = temp.path().join(".copet");
    let manager = manager_with_fake_agents(&root, &home);

    manager.install("gemini").unwrap();
    fs::remove_file(root.join("hooks").join(HELPER_NAME)).unwrap();

    let settings = read_json(home_path(&home, ".gemini/settings.json"));
    let command = settings["hooks"]["BeforeAgent"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    let output = run_hook_shell(command);

    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "{}");
}

#[test]
fn gemini_legacy_install_without_before_agent_is_not_current() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let settings = home_path(&home, ".gemini/settings.json");
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    fs::write(
        &settings,
        r#"{
  "hooks": {
    "BeforeTool": [{
      "matcher": "*",
      "hooks": [{
        "type": "command",
        "command": "if [ -f '/tmp/copet-hook.sh' ]; then '/tmp/copet-hook.sh' gemini tool.before; else echo \"{}\"; fi"
      }]
    }],
    "AfterTool": [{
      "matcher": "*",
      "hooks": [{
        "type": "command",
        "command": "if [ -f '/tmp/copet-hook.sh' ]; then '/tmp/copet-hook.sh' gemini tool.after; else echo \"{}\"; fi"
      }]
    }]
  }
}"#,
    )
    .unwrap();

    let manager = AgentManager::new(temp.path().join(".copet"), home);

    let summary = manager.inspect("gemini").unwrap();

    assert!(!summary.installed);
}
