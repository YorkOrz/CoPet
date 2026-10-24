//! Hook 命令识别的回归测试。
//!
//! 这组用例守住一个真实事故：早期用 `command.contains("copet-hook")` 判断“已安装”，
//! 结果注释里提到 helper、或名字相近的 `copet-hook.sh.bak` 都会被误判成安装完成。

use copet_lib::agents::{command_invokes_copet_helper, HELPER_NAME};

fn unix_helper() -> String {
    format!("/home/u/.copet/hooks/{HELPER_NAME}")
}

fn windows_helper() -> String {
    format!("C://Users//u//.copet//hooks//{HELPER_NAME}")
}

#[test]
fn unix_hook_command_matches_only_its_own_adapter_and_kind() {
    let helper = unix_helper();
    let command = format!(
        "if [ -f '{helper}' ]; then '{helper}' cursor user.prompt; else printf '%s\\n' '{{\"continue\":true}}'; fi"
    );

    assert!(command_invokes_copet_helper(
        &command,
        "cursor",
        Some("user.prompt")
    ));
    assert!(!command_invokes_copet_helper(
        &command,
        "cursor",
        Some("session.stop")
    ));
    assert!(!command_invokes_copet_helper(
        &command,
        "codex",
        Some("user.prompt")
    ));
    assert!(command_invokes_copet_helper(&command, "cursor", None));
}

#[test]
fn windows_cmd_hook_command_matches_and_keeps_fallback_out_of_match() {
    let helper = windows_helper();
    let command = format!(
        "if exist \"{helper}\" (\"{helper}\" antigravity tool.before) else (echo {{\"decision\":\"allow\"}})"
    );

    assert!(command_invokes_copet_helper(
        &command,
        "antigravity",
        Some("tool.before")
    ));
    assert!(!command_invokes_copet_helper(
        &command,
        "antigravity",
        Some("tool.after")
    ));
}

#[test]
fn bare_windows_command_matches() {
    let helper = windows_helper();
    let command = format!("\"{helper}\" workbuddy user.prompt");

    assert!(command_invokes_copet_helper(
        &command,
        "workbuddy",
        Some("user.prompt")
    ));
}

#[test]
fn helper_mentioned_in_comment_is_not_an_install() {
    let helper = unix_helper();
    let command = format!(
        "if [ -f '{helper}' ]; then # {helper} antigravity tool.after\n'/tmp/other.sh' antigravity tool.after; else echo \"{{}}\"; fi"
    );

    assert!(!command_invokes_copet_helper(
        &command,
        "antigravity",
        Some("tool.after")
    ));
}

#[test]
fn helper_name_must_match_exactly() {
    let helper = unix_helper();
    let lookalike = format!("{helper}.bak");
    let command = format!(
        "if [ -f '{lookalike}' ]; then '{lookalike}' cursor user.prompt; else echo \"{{}}\"; fi"
    );

    assert!(!command_invokes_copet_helper(
        &command,
        "cursor",
        Some("user.prompt")
    ));
}

/// Codex 在 Windows 上用 PowerShell 执行 hook，所以安装出来的命令必须是
/// PowerShell 语法。cmd 语法（`if exist "P" (...)`）在 PowerShell 里是
/// `Missing '(' after 'if' in if statement.`，hook 静默失败，宠物就永远停在
/// `Thinking...`。这条用例守住匹配器能认出 PowerShell 写法。
#[test]
fn powershell_hook_command_matches_only_its_own_adapter_and_kind() {
    let helper = windows_helper();
    let command = format!(
        "if (Test-Path \"{helper}\") {{ & \"{helper}\" codex tool.before }} else {{ \"{{}}\" }}"
    );

    assert!(command_invokes_copet_helper(
        &command,
        "codex",
        Some("tool.before")
    ));
    assert!(!command_invokes_copet_helper(
        &command,
        "codex",
        Some("tool.after")
    ));
    assert!(!command_invokes_copet_helper(
        &command,
        "antigravity",
        Some("tool.before")
    ));
    assert!(command_invokes_copet_helper(&command, "codex", None));
}

/// 兜底分支 `"{}"` 里不含 helper 名，不能被误判成一次 helper 调用。
#[test]
fn powershell_fallback_branch_is_not_a_helper_invocation() {
    let helper = windows_helper();
    let command = format!("if (Test-Path \"{helper}\") {{ \"{{}}\" }} else {{ \"{{}}\" }}");

    assert!(!command_invokes_copet_helper(
        &command,
        "codex",
        Some("tool.before")
    ));
    assert!(!command_invokes_copet_helper(&command, "codex", None));
}
