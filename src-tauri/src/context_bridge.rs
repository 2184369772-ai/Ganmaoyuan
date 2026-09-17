//! A deliberately narrow, local-only MCP bridge for one Ganmaoyuan project.
//!
//! The bridge consumes the existing factual `ProjectContextPacket`; it never
//! reads original file bodies, touches project state, or exposes local paths.

use crate::project_service;
use serde_json::{json, Value};
use std::{
    env,
    io::{self, BufRead, Write},
    path::{Path, PathBuf},
};

const SERVER_NAME: &str = "ganmaoyuan-context-bridge";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn run_from_process_args() -> Result<bool, String> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.first().map(String::as_str) != Some("--context-bridge") {
        return Ok(false);
    }
    let root = parse_project_root(&args[1..])?;
    run_stdio_server(&root)?;
    Ok(true)
}

fn parse_project_root(args: &[String]) -> Result<PathBuf, String> {
    if args.len() != 2 || args.first().map(String::as_str) != Some("--project-root") {
        return Err("Context Bridge requires --project-root <path>.".to_string());
    }
    let value = args.get(1).map(String::as_str).unwrap_or_default().trim();
    if value.is_empty() {
        return Err("Context Bridge requires exactly one --project-root value.".to_string());
    }
    let root = PathBuf::from(value);
    if !root.is_dir()
        || !root
            .join(".ganmaoyuan/project-location-manifest.json")
            .is_file()
    {
        return Err(
            "The selected project is unavailable or has no Ganmaoyuan project data.".to_string(),
        );
    }
    root.canonicalize()
        .map_err(|_| "The selected project cannot be opened safely.".to_string())
}

fn run_stdio_server(project_root: &Path) -> Result<(), String> {
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line.map_err(|_| "Context Bridge input closed unexpectedly.".to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Value>(&line) {
            Ok(request) => handle_request(project_root, request),
            Err(_) => Some(json_rpc_error(
                Value::Null,
                -32700,
                "Invalid JSON-RPC request.",
            )),
        };
        if let Some(response) = response {
            let encoded = serde_json::to_string(&response)
                .map_err(|_| "Context Bridge could not encode a response.".to_string())?;
            writeln!(stdout, "{encoded}")
                .and_then(|_| stdout.flush())
                .map_err(|_| "Context Bridge output closed unexpectedly.".to_string())?;
        }
    }
    Ok(())
}

fn handle_request(project_root: &Path, request: Value) -> Option<Value> {
    let id = request.get("id").cloned();
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if method == "notifications/initialized" || id.is_none() {
        return None;
    }
    let id = id.unwrap_or(Value::Null);
    let response = match method {
        "initialize" => Ok(json!({
            "protocolVersion": request
                .pointer("/params/protocolVersion")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("2025-03-26"),
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": { "name": SERVER_NAME, "version": SERVER_VERSION },
            "instructions": "Read-only project facts only. This server is scoped to one local Ganmaoyuan project and never returns original file content or local paths."
        })),
        "tools/list" => Ok(json!({ "tools": bridge_tools() })),
        "tools/call" => call_tool(
            project_root,
            request.get("params").cloned().unwrap_or_default(),
        ),
        "ping" => Ok(json!({})),
        _ => Err((-32601, "Unsupported Context Bridge method.")),
    };
    Some(match response {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err((code, message)) => json_rpc_error(id, code, message),
    })
}

fn bridge_tools() -> Vec<Value> {
    vec![
        tool_definition(
            "get_project_state",
            "Read the current project name, factual focus, freshness summary, and privacy boundary.",
            json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        ),
        tool_definition(
            "get_continue_work_focus",
            "Read the one current factual focus, including its reason and evidence labels. Returns no invented next step when evidence is sparse.",
            json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        ),
        tool_definition(
            "get_recent_activity",
            "Read a bounded timeline of user-visible work facts for this project.",
            json!({
                "type": "object",
                "properties": { "limit": { "type": "integer", "minimum": 1, "maximum": 12 } },
                "additionalProperties": false
            }),
        ),
        tool_definition(
            "get_pending_actions",
            "Read the bounded list of actions that still require a person, with reasons and priority.",
            json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        ),
        tool_definition(
            "get_codex_task_result",
            "Read the most relevant recent Codex task summary and any required manual acceptance. Does not return prompts, run output, or technical logs.",
            json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        ),
    ]
}

fn tool_definition(name: &str, description: &str, input_schema: Value) -> Value {
    json!({ "name": name, "description": description, "inputSchema": input_schema })
}

fn call_tool(project_root: &Path, params: Value) -> Result<Value, (i64, &'static str)> {
    let tool = params
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !bridge_tools()
        .iter()
        .any(|definition| definition.get("name").and_then(Value::as_str) == Some(tool))
    {
        return Err((-32602, "Unknown Context Bridge tool."));
    }
    let packet = project_service::get_project_context_packet(project_root.to_string_lossy().into())
        .map_err(|_| (-32001, "The project facts are temporarily unavailable."))?;
    let arguments = params.get("arguments").cloned().unwrap_or_default();
    let value = match tool {
        "get_project_state" => json!({
            "projectName": packet.project_name,
            "generatedAt": packet.generated_at,
            "focus": packet.focus,
            "sparse": packet.sparse,
            "privacyNotice": packet.privacy_notice
        }),
        "get_continue_work_focus" => json!({ "focus": packet.focus, "sparse": packet.sparse }),
        "get_recent_activity" => {
            let limit = arguments
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(6)
                .clamp(1, 12) as usize;
            json!({ "activity": packet.recent_activity.into_iter().take(limit).collect::<Vec<_>>() })
        }
        "get_pending_actions" => json!({ "pendingActions": packet.pending_actions }),
        "get_codex_task_result" => json!({ "codexTask": packet.codex_result }),
        _ => return Err((-32602, "Unknown Context Bridge tool.")),
    };
    let text = serde_json::to_string_pretty(&value)
        .map_err(|_| (-32001, "The project facts could not be formatted."))?;
    Ok(json!({ "content": [{ "type": "text", "text": text }] }))
}

fn json_rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        models::{ProjectManifest, ProjectSummary},
        project_service::manifest_path,
        storage::write_json_atomic,
    };
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn test_root(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        env::temp_dir().join(format!("ganmaoyuan-context-bridge-{name}-{stamp}"))
    }

    fn setup_project(name: &str) -> PathBuf {
        let root = test_root(name);
        fs::create_dir_all(root.join(".ganmaoyuan")).unwrap();
        let mut manifest = ProjectManifest::default();
        manifest.project = ProjectSummary {
            id: "project-bridge-test".to_string(),
            name: "青岚工作台".to_string(),
            root_dir: root.to_string_lossy().to_string(),
            manifest_path: manifest_path(&root).to_string_lossy().to_string(),
            ..ProjectSummary::default()
        };
        write_json_atomic(&manifest_path(&root), &manifest).unwrap();
        root
    }

    #[test]
    fn tool_list_has_exactly_the_five_read_only_factual_tools() {
        let tools = bridge_tools();
        let names = tools
            .iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert_eq!(names.len(), 5);
        assert!(names.contains(&"get_project_state"));
        assert!(names.contains(&"get_continue_work_focus"));
        assert!(!names
            .iter()
            .any(|name| name.contains("write") || name.contains("import")));
    }

    #[test]
    fn bridge_requires_one_explicit_project_root_argument() {
        assert!(parse_project_root(&[]).is_err());
        assert!(parse_project_root(&["--project-root".to_string()]).is_err());
        assert!(parse_project_root(&[
            "--other".to_string(),
            "value".to_string(),
            "--project-root".to_string(),
            "D:\\project".to_string(),
        ])
        .is_err());
    }

    #[test]
    fn project_state_is_scoped_and_hides_the_local_root() {
        let root = setup_project("scoped");
        let response = handle_request(
            &root,
            json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": "get_project_state", "arguments": {} } }),
        )
        .unwrap();
        let text = response
            .pointer("/result/content/0/text")
            .and_then(Value::as_str)
            .unwrap();
        assert!(text.contains("青岚工作台"));
        assert!(!text.contains(&root.to_string_lossy().to_string()));
        assert!(!text.contains("repositoryPath"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unknown_tool_returns_a_safe_json_rpc_error() {
        let root = setup_project("unknown-tool");
        let response = handle_request(
            &root,
            json!({ "jsonrpc": "2.0", "id": "call-1", "method": "tools/call", "params": { "name": "write_project", "arguments": {} } }),
        )
        .unwrap();
        assert_eq!(
            response.pointer("/error/code").and_then(Value::as_i64),
            Some(-32602)
        );
        assert!(!response
            .to_string()
            .contains(&root.to_string_lossy().to_string()));
        fs::remove_dir_all(root).unwrap();
    }
}
