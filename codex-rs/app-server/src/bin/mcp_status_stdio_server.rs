use std::io::BufRead;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use serde_json::Value;
use serde_json::json;

const DYNAMIC_SERVER_METADATA_ENV: &str = "MCP_TEST_DYNAMIC_SERVER_METADATA";
const INITIALIZE_BARRIER_FILE_ENV: &str = "MCP_TEST_INITIALIZE_BARRIER_FILE";
const PID_FILE_ENV: &str = "MCP_TEST_PID_FILE";

fn main() -> Result<()> {
    if let Ok(pid_file) = std::env::var(PID_FILE_ENV) {
        std::fs::write(pid_file, std::process::id().to_string())?;
    }

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let request: Value = serde_json::from_str(&line?)?;
        let Some(method) = request.get("method").and_then(Value::as_str) else {
            continue;
        };
        let Some(id) = request.get("id").cloned() else {
            continue;
        };
        let result = match method {
            "initialize" => initialize_result(&request),
            "tools/list" => tools_list_result(),
            _ => continue,
        };
        serde_json::to_writer(
            &mut stdout,
            &json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": result,
            }),
        )?;
        stdout.write_all(b"\n")?;
        stdout.flush()?;
    }

    Ok(())
}

fn initialize_result(request: &Value) -> Value {
    if let Ok(barrier_file) = std::env::var(INITIALIZE_BARRIER_FILE_ENV) {
        while !Path::new(&barrier_file).is_file() {
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    let process_label = dynamic_server_process_label();
    json!({
        "protocolVersion": request
            .pointer("/params/protocolVersion")
            .and_then(Value::as_str)
            .unwrap_or("2025-06-18"),
        "capabilities": {
            "tools": {}
        },
        "serverInfo": {
            "name": "codex-app-server-test-mcp-status-stdio-server",
            "title": process_label,
            "version": env!("CARGO_PKG_VERSION")
        }
    })
}

fn tools_list_result() -> Value {
    let process_label = dynamic_server_process_label();
    json!({
        "tools": [{
            "name": "echo",
            "description": format!("Echo from {process_label}."),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string"
                    }
                },
                "required": ["message"]
            }
        }]
    })
}

fn dynamic_server_process_label() -> String {
    assert!(
        std::env::var_os(DYNAMIC_SERVER_METADATA_ENV).is_some(),
        "{DYNAMIC_SERVER_METADATA_ENV} must be set"
    );
    format!("rmcp-test-process-{}", std::process::id())
}
