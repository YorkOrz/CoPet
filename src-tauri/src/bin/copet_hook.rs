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

    // Read stdin with 150ms timeout so it never blocks
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buf = String::new();
        let _ = std::io::stdin().read_to_string(&mut buf);
        let _ = tx.send(buf);
    });
    let input = rx
        .recv_timeout(Duration::from_millis(150))
        .unwrap_or_default();

    let parsed: Option<Value> = serde_json::from_str(&input).ok();

    let tool_name = parsed.as_ref().and_then(|v| {
        v.get("tool_name")
            .or_else(|| v.get("toolName"))
            .or_else(|| v.get("tool"))
            .or_else(|| v.get("name"))
            .or_else(|| v.get("toolCall").and_then(|tc| tc.get("name")))
            .and_then(Value::as_str)
            .map(String::from)
    });

    let tool_input = parsed.as_ref().and_then(|v| {
        v.get("tool_input")
            .or_else(|| v.get("toolInput"))
            .or_else(|| v.get("input"))
            .or_else(|| v.get("arguments"))
            .or_else(|| v.get("toolCall").and_then(|tc| tc.get("args")))
            .cloned()
    });

    let mut mapped_kind = kind.to_string();
    let mut mapped_tool = tool_name.clone();

    if let Some(ref t) = tool_name {
        match t.as_str() {
            "view_file"
            | "read_url_content"
            | "search_web"
            | "list_resources"
            | "read_resource"
            | "view_image"
            | "list_mcp_resources"
            | "list_mcp_resource_templates"
            | "read_mcp_resource" => {
                mapped_tool = Some("read".to_string());
            }
            "replace_file_content" | "write_to_file" | "run_command" | "invoke_subagent" => {
                mapped_tool = Some("edit".to_string());
            }
            "ask_question" | "request_user_input" => {
                mapped_kind = "permission.waiting".to_string();
            }
            _ => {}
        }
    }

    let _ = send_event(
        agent,
        &mapped_kind,
        mapped_tool.as_deref(),
        tool_input.as_ref(),
    );

    if agent == "antigravity" && (kind == "tool.before" || kind == "session.stop") {
        println!(r#"{{"decision":"allow"}}"#);
    } else if agent == "cursor" && kind == "user.prompt" {
        println!(r#"{{"continue":true}}"#);
    } else {
        println!("{{}}");
    }
}

fn send_event(
    agent: &str,
    kind: &str,
    tool: Option<&str>,
    tool_input: Option<&Value>,
) -> Result<(), ()> {
    let home = dirs::home_dir().ok_or(())?;
    let runtime_dir = home.join(".copet").join("runtime");
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
