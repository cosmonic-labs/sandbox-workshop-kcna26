//! talks-mcp: a small MCP server over the conference talks database.
//!
//! It speaks MCP's Streamable HTTP transport at the 2026-07-28 revision, the
//! stateless one: every request is one POST carrying a JSON-RPC message, and
//! the answer is one JSON reply. It offers one tool, `talks_sql`, and runs it
//! through the sqlite tool, composed in with it (toolset.wac), so the server
//! can read only what the manifest mounts for it: the talks database, read
//! only. It holds no folder, host or socket of its own.
//!
//! An agent reaches it through Cosmonic's MCP bridge, the same bridge
//! Cosmonic Desktop runs, which turns this server's tools into the agent
//! SDK's `tools`. Written by hand to stay small; to build a real MCP server
//! as a component, start from Cosmonic's rust-mcp template (the official
//! `rmcp` SDK).

wit_bindgen::generate!({
    path: "../../wit",
    world: "workshop:toolbox/talks-mcp",
    generate_all,
});

#[path = "../../common/http.rs"]
mod http;

use exports::wasi::http::handler::Guest;
use workshop::toolbox::sqlite;
use serde_json::{Value, json};
use wasi::http::types::{ErrorCode, Method, Request, Response};

/// The revision this server speaks.
const PROTOCOL: &str = "2026-07-28";

struct TalksMcp;

/// The one tool, as MCP's tools/list describes it.
fn tool() -> Value {
    json!({
        "name": "talks_sql",
        "title": "Talks database",
        "description": "Run read-only SQL against the conference talks database and return \
                        the rows, one per line. Also accepts .tables and .schema.",
        "inputSchema": {
            "type": "object",
            "properties": { "sql": { "type": "string", "description": "SQL to run, such as select title from talks limit 5." } },
            "required": ["sql"],
        },
        "annotations": { "readOnlyHint": true, "openWorldHint": false },
    })
}

/// Answer one JSON-RPC request: the result, or a JSON-RPC error (code, message).
async fn answer(method: &str, params: &Value) -> Result<Value, (i64, String)> {
    match method {
        "tools/list" => Ok(json!({ "tools": [tool()] })),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or_default();
            if name != "talks_sql" {
                return Err((-32602, format!("unknown tool {name}")));
            }
            let sql = params
                .pointer("/arguments/sql")
                .and_then(Value::as_str)
                .ok_or((-32602, "missing argument sql".to_string()))?;
            // The tool's own errors are a completed call that failed: isError,
            // with the error as `{"case": "payload"}`, as the workshop's other
            // tools report theirs.
            let (text, is_error) = match sqlite::query(sqlite::QueryArgs { sql: sql.into() }).await {
                Ok(rows) => (rows, false),
                Err(e) => (sqlite_error(e), true),
            };
            Ok(json!({ "content": [{ "type": "text", "text": text }], "isError": is_error }))
        }
        "ping" => Ok(json!({})),
        other => Err((-32601, format!("method not found: {other}"))),
    }
}

/// A sqlite error as `{"case": "payload"}`.
fn sqlite_error(e: sqlite::Error) -> String {
    use sqlite::Error::*;
    let (case, payload) = match e {
        InvalidArgument(m) => ("invalid-argument", Some(m)),
        AccessDenied(m) => ("access-denied", Some(m)),
        NotFound(m) => ("not-found", Some(m)),
        Other(m) => ("other", m),
    };
    json!({ case: payload }).to_string()
}

impl Guest for TalksMcp {
    async fn handle(req: Request) -> Result<Response, ErrorCode> {
        if !matches!(req.get_method(), Method::Post) {
            return http::text_response(405, "talks-mcp: POST a JSON-RPC message to /mcp\n".into());
        }
        let body = http::request_body(req).await;
        let Ok(msg) = serde_json::from_slice::<Value>(&body) else {
            return http::json_response(400, json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": "parse error" } }).to_string());
        };
        // A notification has no id and gets no reply.
        let Some(id) = msg.get("id").cloned() else {
            return http::text_response(202, String::new());
        };
        let method = msg.get("method").and_then(Value::as_str).unwrap_or_default();
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        let reply = match answer(method, &params).await {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err((code, message)) => json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message, "data": { "supported": [PROTOCOL] } } }),
        };
        http::json_response(200, reply.to_string())
    }
}

export!(TalksMcp);
