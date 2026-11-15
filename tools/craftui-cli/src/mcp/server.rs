use super::prompts::{get_prompt, list_prompts};
use super::protocol::{
    INVALID_PARAMS, InitializeResult, JsonRpcRequest, JsonRpcResponse, METHOD_NOT_FOUND,
    PARSE_ERROR, PROTOCOL_VERSION, PromptsCapability, ResourcesCapability, SERVER_NAME,
    SERVER_VERSION, ServerCapabilities, ServerInfo, ToolsCapability,
};
use super::resources::{list_resources, read_resource};
use super::tools::{call_tool, list_tools};
use craftui_core::Result;
use serde_json::{Value, json};
use std::io::{BufRead, Write};

pub fn run_stdio_server() -> Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut reader = stdin.lock();
    let mut writer = stdout.lock();

    let mut line = String::new();
    while reader.read_line(&mut line)? > 0 {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            line.clear();
            continue;
        }

        let resp = match serde_json::from_str::<JsonRpcRequest>(trimmed) {
            Ok(req) => handle_request(req),
            Err(e) => Some(JsonRpcResponse::error(
                Value::Null,
                PARSE_ERROR,
                format!("JSON parse error: {e}"),
            )),
        };

        if let Some(res) = resp {
            let json_str = serde_json::to_string(&res).map_err(|e| {
                craftui_core::CraftError::Platform(format!("Failed to serialize response: {e}"))
            })?;
            writeln!(writer, "{json_str}")?;
            writer.flush()?;
        }

        line.clear();
    }

    Ok(())
}

fn handle_request(req: JsonRpcRequest) -> Option<JsonRpcResponse> {
    // If id is null or missing, it's a notification
    let id = match req.id {
        Some(i) => i,
        None => {
            // Notification: no response required
            return None;
        }
    };

    match req.method.as_str() {
        "initialize" => {
            let res = InitializeResult {
                protocol_version: PROTOCOL_VERSION.to_string(),
                capabilities: ServerCapabilities {
                    tools: ToolsCapability {
                        list_changed: false,
                    },
                    resources: ResourcesCapability {
                        subscribe: false,
                        list_changed: false,
                    },
                    prompts: PromptsCapability {
                        list_changed: false,
                    },
                },
                server_info: ServerInfo {
                    name: SERVER_NAME.to_string(),
                    version: SERVER_VERSION.to_string(),
                },
            };
            Some(JsonRpcResponse::success(
                id,
                serde_json::to_value(res).unwrap_or(Value::Null),
            ))
        }
        "ping" => Some(JsonRpcResponse::success(id, json!({}))),
        "tools/list" => {
            let tools = list_tools();
            Some(JsonRpcResponse::success(id, json!({ "tools": tools })))
        }
        "tools/call" => {
            let params = req.params.unwrap_or(Value::Null);
            let tool_name = params.get("name").and_then(Value::as_str);
            let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

            match tool_name {
                Some(name) => {
                    let result = call_tool(name, arguments);
                    Some(JsonRpcResponse::success(
                        id,
                        serde_json::to_value(result).unwrap_or(Value::Null),
                    ))
                }
                None => Some(JsonRpcResponse::error(
                    id,
                    INVALID_PARAMS,
                    "missing required 'name' in tools/call parameters",
                )),
            }
        }
        "resources/list" => {
            let resources = list_resources();
            Some(JsonRpcResponse::success(
                id,
                json!({ "resources": resources }),
            ))
        }
        "resources/read" => {
            let params = req.params.unwrap_or(Value::Null);
            let uri = params.get("uri").and_then(Value::as_str);
            match uri {
                Some(u) => match read_resource(u) {
                    Some(content) => Some(JsonRpcResponse::success(
                        id,
                        json!({ "contents": [content] }),
                    )),
                    None => Some(JsonRpcResponse::error(
                        id,
                        INVALID_PARAMS,
                        format!("resource not found: '{u}'"),
                    )),
                },
                None => Some(JsonRpcResponse::error(
                    id,
                    INVALID_PARAMS,
                    "missing required 'uri' parameter",
                )),
            }
        }
        "prompts/list" => {
            let prompts = list_prompts();
            Some(JsonRpcResponse::success(id, json!({ "prompts": prompts })))
        }
        "prompts/get" => {
            let params = req.params.unwrap_or(Value::Null);
            let prompt_name = params.get("name").and_then(Value::as_str);
            let arguments = params.get("arguments").unwrap_or(&Value::Null);

            match prompt_name {
                Some(name) => match get_prompt(name, arguments) {
                    Some(messages) => Some(JsonRpcResponse::success(
                        id,
                        json!({ "messages": messages }),
                    )),
                    None => Some(JsonRpcResponse::error(
                        id,
                        INVALID_PARAMS,
                        format!("prompt not found: '{name}'"),
                    )),
                },
                None => Some(JsonRpcResponse::error(
                    id,
                    INVALID_PARAMS,
                    "missing required 'name' parameter",
                )),
            }
        }
        unknown => Some(JsonRpcResponse::error(
            id,
            METHOD_NOT_FOUND,
            format!("method '{unknown}' not found"),
        )),
    }
}
