//! talks-mcp: an MCP server for the talks service, as an ordinary HTTP
//! component. It answers MCP's JSON-RPC over HTTP (initialize, tools/list,
//! tools/call) at /mcp and turns each tool call into a REST request to the
//! talks service (components/talks-api).
//!
//! Its manifest grants it one host, the talks service. Every request it
//! makes goes through wasi:http, where the host checks that grant before
//! anything leaves, so a tool here that is told to fetch somewhere else
//! (import_talks, pointed at another host) is refused by the host, not by
//! this code.

wit_bindgen::generate!({
    path: "../../wit",
    world: "workshop:toolbox/talks-mcp",
    generate_all,
});

#[path = "../../common/http.rs"]
mod http;
#[path = "../../common/http_client.rs"]
mod http_client;

use exports::wasi::http::handler::Guest;
use http_client::SendError;
use serde_json::{Value, json};
use wasi::http::types::{ErrorCode, Method, Request, Response};

/// The talks service, the one host this server is granted.
const API: &str = "http://talks.mock-sites.svc";
/// The MCP revision this server speaks when a client doesn't ask for one.
const PROTOCOL: &str = "2025-06-18";

fn tools() -> Value {
    json!([
        {
            "name": "list_talks",
            "description": "List the conference talks in schedule order: id, title, speaker, track, room and time. Optionally only one track (Security, Wasm or Platform).",
            "inputSchema": { "type": "object", "properties": { "track": { "type": "string", "description": "Only this track, such as Security." } } },
            "annotations": { "readOnlyHint": true }
        },
        {
            "name": "get_talk",
            "description": "One talk by id, and whether it is a favorite.",
            "inputSchema": { "type": "object", "properties": { "id": { "type": "integer" } }, "required": ["id"] },
            "annotations": { "readOnlyHint": true }
        },
        {
            "name": "favorite_talk",
            "description": "Mark a talk as a favorite.",
            "inputSchema": { "type": "object", "properties": { "id": { "type": "integer" } }, "required": ["id"] },
            "annotations": { "readOnlyHint": false, "idempotentHint": true }
        },
        {
            "name": "import_talks",
            "description": "Read a list of talks from another schedule at a URL.",
            "inputSchema": { "type": "object", "properties": { "url": { "type": "string" } }, "required": ["url"] },
            "annotations": { "readOnlyHint": true, "openWorldHint": true }
        }
    ])
}

/// Why a request to `url` produced no response, in words for the model.
fn send_error(url: &str, e: SendError) -> String {
    let host = url.split("://").nth(1).unwrap_or(url).split(['/', '?']).next().unwrap_or(url);
    match e {
        SendError::Http(ErrorCode::HttpRequestDenied) => {
            format!("{host}: this server may not reach that host (not in its allowedHosts)")
        }
        other => format!("{host}: {other}"),
    }
}

/// One REST request to the talks service, as (ok, text).
async fn rest(method: &str, path: &str) -> Result<Value, String> {
    let url = format!("{API}{path}");
    let (status, body) = http_client::send(method, &url, &[("accept", "application/json")], Vec::new())
        .await
        .map_err(|e| send_error(&url, e))?;
    let v: Value = serde_json::from_slice(&body).map_err(|_| format!("talks service: HTTP {status}"))?;
    if status >= 400 {
        return Err(v["error"].as_str().map_or(format!("talks service: HTTP {status}"), String::from));
    }
    Ok(v)
}

fn line(t: &Value) -> String {
    format!(
        "{} | {} | {} | {} | {} | {} {}",
        t["id"], t["title"].as_str().unwrap_or(""), t["speaker"].as_str().unwrap_or(""),
        t["track"].as_str().unwrap_or(""), t["room"].as_str().unwrap_or(""),
        t["day"].as_str().unwrap_or(""), t["start"].as_str().unwrap_or("")
    )
}

/// Run one tool; Err is a tool result with isError set.
async fn call(name: &str, args: &Value) -> Result<String, String> {
    let id = || args["id"].as_u64().ok_or_else(|| format!("{name}: id must be a number"));
    match name {
        "list_talks" => {
            let path = match args["track"].as_str() {
                Some(t) => format!("/talks?track={}", t.replace(' ', "%20")),
                None => "/talks".to_string(),
            };
            let v = rest("GET", &path).await?;
            Ok(v.as_array().map(|a| a.iter().map(line).collect::<Vec<_>>().join("\n")).unwrap_or_default())
        }
        "get_talk" => Ok(rest("GET", &format!("/talks/{}", id()?)).await?.to_string()),
        "favorite_talk" => Ok(rest("PUT", &format!("/talks/{}/favorite", id()?)).await?.to_string()),
        "import_talks" => {
            let url = args["url"].as_str().ok_or_else(|| "import_talks: url is required".to_string())?;
            let (status, body) = http_client::send("GET", url, &[], Vec::new())
                .await
                .map_err(|e| format!("import_talks: {}", send_error(url, e)))?;
            Ok(format!("import_talks: HTTP {status}, {} bytes; nothing imported", body.len()))
        }
        other => Err(format!("no tool named {other}")),
    }
}

/// One JSON-RPC message in, its answer out (None for a notification).
async fn rpc(msg: &Value) -> Option<Value> {
    let id = msg.get("id")?.clone();
    let params = &msg["params"];
    let result = match msg["method"].as_str().unwrap_or("") {
        "initialize" => json!({
            "protocolVersion": params["protocolVersion"].as_str().unwrap_or(PROTOCOL),
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "talks-mcp", "version": "0.1.0" }
        }),
        "ping" => json!({}),
        "tools/list" => json!({ "tools": tools() }),
        "tools/call" => {
            let name = params["name"].as_str().unwrap_or("");
            let (text, is_error) = match call(name, &params["arguments"]).await {
                Ok(t) => (t, false),
                Err(t) => (t, true),
            };
            json!({ "content": [{ "type": "text", "text": text }], "isError": is_error })
        }
        m => {
            return Some(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": format!("method not found: {m}") } }));
        }
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

struct Server;

impl Guest for Server {
    async fn handle(req: Request) -> Result<Response, ErrorCode> {
        let path = req.get_path_with_query().unwrap_or_default();
        if path.split('?').next() != Some("/mcp") {
            return http::text_response(404, format!("talks-mcp: MCP is at /mcp, not {path}\n"));
        }
        if !matches!(req.get_method(), Method::Post) {
            return http::text_response(405, "talks-mcp: POST JSON-RPC to /mcp\n".to_string());
        }
        let body = http::request_body(req).await;
        let Ok(msg) = serde_json::from_slice::<Value>(&body) else {
            let e = json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": "parse error" } });
            return http::json_response(400, e.to_string());
        };
        match rpc(&msg).await {
            Some(answer) => http::json_response(200, answer.to_string()),
            None => http::text_response(202, String::new()),
        }
    }
}

export!(Server);
