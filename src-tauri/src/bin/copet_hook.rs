use serde_json::{json, Value};
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

fn main() {
    let args: Vec<String> = env::args().collect();
    let agent = args.get(1).map(|s| s.as_str()).unwrap_or("antigravity");
    let kind = args.get(2).map(|s| s.as_str()).unwrap_or("unknown");

    // hook 执行器写完 stdin 会关闭管道，正常路径立刻拿到 EOF；超时只是兜底，
    // 放宽到 1.5s，避免 Windows 上进程调度慢时把 payload 截断。
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buf = String::new();
        let _ = std::io::stdin().read_to_string(&mut buf);
        let _ = tx.send(buf);
    });
    let input = rx
        .recv_timeout(Duration::from_millis(1500))
        .unwrap_or_default();

    let parsed: Option<Value> = serde_json::from_str(&input).ok();

    // Claude Code 也会收到 Cursor 兼容 payload（带 cursor_version），此时不该再发一次事件。
    if agent == "claude-code"
        && parsed
            .as_ref()
            .is_some_and(|value| value.get("cursor_version").is_some())
    {
        hook_output(agent, kind);
        return;
    }

    let tool = parsed.as_ref().and_then(|value| {
        value
            .get("tool_name")
            .or_else(|| value.get("toolName"))
            .or_else(|| value.get("tool"))
            .or_else(|| value.get("name"))
            .or_else(|| value.get("toolCall").and_then(|call| call.get("name")))
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
            .map(String::from)
    });

    // 与 shell helper 保持一致：只把第一个命中的字段转成 `{输出字段: 值}`，
    // 原始 tool 名不做映射，状态判定统一交给 runtime_state。
    let tool_input = parsed
        .as_ref()
        .and_then(|value| {
            value
                .get("tool_input")
                .or_else(|| value.get("toolInput"))
                .or_else(|| value.get("input"))
                .or_else(|| value.get("arguments"))
                .or_else(|| value.get("toolCall").and_then(|call| call.get("args")))
        })
        .and_then(map_tool_input);

    let _ = send_event(agent, kind, tool.as_deref(), tool_input.as_ref());

    hook_output(agent, kind);
}

fn hook_output(agent: &str, kind: &str) {
    if agent == "antigravity" && (kind == "tool.before" || kind == "session.stop") {
        println!(r#"{{"decision":"allow"}}"#);
    } else if agent == "cursor" && kind == "user.prompt" {
        println!(r#"{{"continue":true}}"#);
    } else {
        println!("{{}}");
    }
}

/// 与 shell helper 的字段映射表保持一致（源字段 -> 输出字段，按优先级排列）。
const TOOL_INPUT_FIELDS: &[(&str, &str)] = &[
    ("file_path", "file_path"),
    ("filePath", "filePath"),
    ("file", "file"),
    ("path", "path"),
    ("command", "command"),
    ("CommandLine", "command"),
    ("pattern", "pattern"),
    ("url", "url"),
    ("description", "description"),
    ("subject", "subject"),
    ("prompt", "subject"),
    ("message", "subject"),
    ("error", "subject"),
    ("initialPrompt", "subject"),
    ("AbsolutePath", "filePath"),
    ("TargetFile", "filePath"),
    ("DirectoryPath", "path"),
    ("SearchDirectory", "path"),
    ("SearchPath", "path"),
    ("Cwd", "path"),
    ("Query", "pattern"),
    ("query", "pattern"),
    ("Pattern", "pattern"),
    ("Url", "url"),
    ("Description", "subject"),
    ("Instruction", "subject"),
    ("Prompt", "subject"),
    ("Input", "subject"),
    ("Message", "subject"),
    ("Reason", "subject"),
    ("Action", "subject"),
    ("Target", "subject"),
    ("ImageName", "subject"),
];

fn map_tool_input(value: &Value) -> Option<Value> {
    let object = value.as_object()?;
    for (source, target) in TOOL_INPUT_FIELDS {
        if let Some(text) = object.get(*source).and_then(Value::as_str) {
            if !text.is_empty() {
                return Some(json!({ *target: text }));
            }
        }
    }
    None
}

fn send_event(
    agent: &str,
    kind: &str,
    tool: Option<&str>,
    tool_input: Option<&Value>,
) -> Result<(), ()> {
    let runtime_dir = std::env::var_os("COPET_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .or_else(|| dirs::home_dir().map(|home| home.join(".copet").join("runtime")))
        .ok_or(())?;
    let endpoint = fs::read_to_string(runtime_dir.join("event-endpoint")).map_err(|_| ())?;
    let token = fs::read_to_string(runtime_dir.join("event-token")).map_err(|_| ())?;

    let endpoint = endpoint.trim();
    let token = token.trim();
    if endpoint.is_empty() || token.is_empty() {
        return Err(());
    }

    let stripped = endpoint.strip_prefix("http://").unwrap_or(endpoint);
    let host_port_path: Vec<&str> = stripped.splitn(2, '/').collect();
    let host_port = host_port_path[0];
    let path = if host_port_path.len() > 1 {
        format!("/{}", host_port_path[1])
    } else {
        "/v1/events".to_string()
    };

    let mut body = json!({
        "agent": agent,
        "kind": kind,
    });
    if let Some(t) = tool {
        body["tool"] = json!(t);
    }
    if let Some(ti) = tool_input {
        body["toolInput"] = ti.clone();
    }
    let payload = serde_json::to_string(&body).map_err(|_| ())?;

    let socket_addr: std::net::SocketAddr = host_port.parse().map_err(|_| ())?;
    let mut stream =
        TcpStream::connect_timeout(&socket_addr, Duration::from_millis(80)).map_err(|_| ())?;

    let _ = stream.set_write_timeout(Some(Duration::from_millis(80)));
    let _ = stream.set_read_timeout(Some(Duration::from_millis(80)));

    let request = format!(
        "POST {} HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        path,
        host_port,
        token,
        payload.len(),
        payload
    );

    let _ = stream.write_all(request.as_bytes());
    Ok(())
}
