#![allow(dead_code)]

use copet_lib::agents::AgentManager;
use serde_json::Value;
use std::{env, ffi::OsString, fs, path::PathBuf, process::Stdio, sync::Mutex};

#[cfg(windows)]
pub const HELPER_NAME: &str = "copet-hook.exe";
#[cfg(not(windows))]
pub const HELPER_NAME: &str = "copet-hook.sh";

pub static OPENCODE_ENV_LOCK: Mutex<()> = Mutex::new(());
pub static COPILOT_ENV_LOCK: Mutex<()> = Mutex::new(());

/// 按 `/` 拆段拼接，避免 `join("a/b")` 在 Windows 上留下混合分隔符，
/// 与 adapter 用多段 join 生成的路径对不上。
pub fn home_path(base: impl AsRef<std::path::Path>, relative: &str) -> PathBuf {
    relative
        .split('/')
        .fold(base.as_ref().to_path_buf(), |path, segment| {
            path.join(segment)
        })
}

/// 用平台默认 shell 执行一条 hook 命令（unix 走 sh，Windows 走 cmd）。
/// Windows 上必须用 `raw_arg` 追加命令：普通 `arg` 会把命令里的 `"` 转义成 `\"`，
/// cmd 不认这种转义，兜底输出就会变成 `{\"continue\":true}`。
pub fn hook_shell(command: &str) -> std::process::Command {
    #[cfg(windows)]
    let mut shell = {
        use std::os::windows::process::CommandExt;

        let mut shell = std::process::Command::new("cmd");
        shell.raw_arg(format!("/c {command}"));
        shell
    };
    #[cfg(not(windows))]
    let mut shell = {
        let mut shell = std::process::Command::new("sh");
        shell.arg("-c");
        shell.arg(command);
        shell
    };

    shell
}

pub fn run_hook_shell(command: &str) -> std::process::Output {
    hook_shell(command)
        .stdin(Stdio::null())
        .output()
        .expect("hook shell should run")
}

struct EnvRestore {
    key: &'static str,
    value: Option<OsString>,
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        restore_env_var(self.key, self.value.clone());
    }
}

pub fn read_json(path: impl AsRef<std::path::Path>) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

pub fn manager_with_fake_agents(
    root: impl Into<PathBuf>,
    home: impl Into<PathBuf>,
) -> AgentManager {
    manager_with_fake_agent_names(
        root,
        home,
        &[
            "claude",
            "codex",
            "agy",
            "gemini",
            "opencode",
            "copilot",
            "cursor",
            "pi",
            "workbuddy",
        ],
    )
}

/// Windows 上 `ensure_helper` 从当前可执行文件同目录复制 `copet-hook.exe`。
/// 测试二进制在 `target/debug/deps/`，真 helper 在 `target/debug/`，这里先补一份过去。
#[cfg(windows)]
fn stage_helper_binary() {
    static STAGE: std::sync::Once = std::sync::Once::new();
    STAGE.call_once(|| {
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        let Some(dir) = exe.parent() else {
            return;
        };
        let staged = dir.join(HELPER_NAME);
        let parent = dir.parent().unwrap_or(dir);
        // 总是覆盖：`cargo test` 只重建 bin，deps 里的残留副本会是旧版本。
        let _ = fs::copy(parent.join(HELPER_NAME), &staged);
    });
}

pub fn manager_with_fake_agent_names(
    root: impl Into<PathBuf>,
    home: impl Into<PathBuf>,
    executables: &[&str],
) -> AgentManager {
    #[cfg(windows)]
    stage_helper_binary();
    let temp = tempfile::tempdir().unwrap();
    let bin = temp.keep().join("bin");
    fs::create_dir_all(&bin).unwrap();
    for executable in executables {
        let path = bin.join(executable);
        fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&path).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&path, permissions).unwrap();
        }
    }
    AgentManager::new_with_exact_executable_search_paths(root, home, vec![bin])
}

pub fn with_opencode_config_dir(test: impl FnOnce(&std::path::Path)) {
    let _guard = OPENCODE_ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let temp = tempfile::tempdir().unwrap();
    let opencode_config_dir = temp.path().join("opencode-config");
    let previous = env::var_os("OPENCODE_CONFIG_DIR");

    env::set_var("OPENCODE_CONFIG_DIR", &opencode_config_dir);
    test(&opencode_config_dir);
    restore_env_var("OPENCODE_CONFIG_DIR", previous);
}

pub fn with_copilot_home(test: impl FnOnce(&std::path::Path)) {
    let _guard = COPILOT_ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let temp = tempfile::tempdir().unwrap();
    let copilot_home = temp.path().join("copilot-home");
    let _restore = EnvRestore {
        key: "COPILOT_HOME",
        value: env::var_os("COPILOT_HOME"),
    };

    env::set_var("COPILOT_HOME", &copilot_home);
    test(&copilot_home);
}

pub fn with_cleared_copilot_home(test: impl FnOnce()) {
    let _guard = COPILOT_ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _restore = EnvRestore {
        key: "COPILOT_HOME",
        value: env::var_os("COPILOT_HOME"),
    };

    env::remove_var("COPILOT_HOME");
    test();
}

pub fn with_empty_copilot_home(test: impl FnOnce()) {
    let _guard = COPILOT_ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _restore = EnvRestore {
        key: "COPILOT_HOME",
        value: env::var_os("COPILOT_HOME"),
    };

    env::set_var("COPILOT_HOME", "");
    test();
}

pub fn restore_env_var(key: &str, value: Option<OsString>) {
    if let Some(value) = value {
        env::set_var(key, value);
    } else {
        env::remove_var(key);
    }
}
