//! mcp: call the tools of an MCP server over HTTP, from a command line.
//!
//!   mcp tools                         list the server's tools
//!   mcp call <tool> <json>            call one, such as
//!                                     call list_talks {"track":"Security"}
//!   mcp --server <url> ...            another server than the talks one
//!
//! This tool is only an MCP client. Every request goes through wasi:http,
//! where the host checks it against this component's allowedHosts (the talks
//! MCP server) before anything leaves. What a server's tools can reach is
//! the server's own grant: this tool never sees the service behind it.

wit_bindgen::generate!({
    path: "../../wit",
    world: "workshop:toolbox/mcp-tool",
    generate_all,
});

#[path = "../../common/http_client.rs"]
mod http_client;

use exports::workshop::toolbox::mcp::{Error, Guest, RunArgs};
use http_client::SendError;
use serde_json::{Value, json};
use wasi::http::types::ErrorCode;

/// The talks MCP server, the one this tool is granted in the steps.
const DEFAULT_SERVER: &str = "http://mcp.mock-sites.svc/mcp";
const PROTOCOL: &str = "2025-06-18";

fn host_of(url: &str) -> &str {
    url.split("://").nth(1).unwrap_or(url).split(['/', '?']).next().unwrap_or(url)
}

fn send_error(url: &str, e: SendError) -> Error {
    let host = host_of(url);
    match e {
        SendError::Invalid(m) => Error::InvalidArgument(m),
        SendError::Http(ErrorCode::HttpRequestDenied) => Error::AccessDenied(format!(
            "{host}: this tool may not reach that host (not in its allowedHosts)"
        )),
        SendError::Http(code) => Error::Network(format!("{host}: {code:?}")),
    }
}

/// An MCP session over plain HTTP: initialize, then requests.
struct Session {
    url: String,
    session_id: Option<String>,
    next_id: u64,
}

impl Session {
    async fn post(&mut self, msg: Value) -> Result<Option<Value>, Error> {
        let body = msg.to_string().into_bytes();
        let sid = self.session_id.clone();
        let mut headers = vec![
            ("content-type", "application/json"),
            ("accept", "application/json, text/event-stream"),
            ("mcp-protocol-version", PROTOCOL),
        ];
        if let Some(s) = sid.as_deref() {
            headers.push(("mcp-session-id", s));
        }
        let (status, resp_headers, bytes) = http_client::send_full("POST", &self.url, &headers, body)
            .await
            .map_err(|e| send_error(&self.url, e))?;
        if let Some((_, v)) = resp_headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("mcp-session-id")) {
            self.session_id = Some(String::from_utf8_lossy(v).into_owned());
        }
        if status == 202 || bytes.is_empty() {
            return Ok(None);
        }
        let text = String::from_utf8_lossy(&bytes);
        // A server may answer as an event stream: take its last data line.
        let json_text = if text.trim_start().starts_with('{') {
            text.to_string()
        } else {
            text.lines().rev().find_map(|l| l.strip_prefix("data:")).unwrap_or("").trim().to_string()
        };
        let v: Value = serde_json::from_str(&json_text).map_err(|_| {
            Error::Network(format!("{}: HTTP {status}, not an MCP answer", host_of(&self.url)))
        })?;
        if let Some(err) = v.get("error") {
            return Err(Error::Other(Some(format!(
                "{}: {}",
                host_of(&self.url),
                err["message"].as_str().unwrap_or("error")
            ))));
        }
        Ok(Some(v["result"].clone()))
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value, Error> {
        self.next_id += 1;
        let msg = json!({ "jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params });
        self.post(msg).await?.ok_or_else(|| Error::Network(format!("{}: no answer to {method}", host_of(&self.url))))
    }

    async fn open(url: String) -> Result<Self, Error> {
        let mut s = Session { url, session_id: None, next_id: 0 };
        s.request(
            "initialize",
            json!({ "protocolVersion": PROTOCOL, "capabilities": {}, "clientInfo": { "name": "workshop-mcp-tool", "version": "0.1.0" } }),
        )
        .await?;
        s.post(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).await?;
        Ok(s)
    }
}

async fn run(arguments: &str) -> Result<String, Error> {
    let mut rest = arguments.trim();
    let mut server = DEFAULT_SERVER.to_string();
    if let Some(r) = rest.strip_prefix("--server") {
        let r = r.trim_start();
        let (url, after) = r.split_once(char::is_whitespace).unwrap_or((r, ""));
        if url.is_empty() {
            return Err(Error::InvalidArgument("--server needs a URL".into()));
        }
        server = url.to_string();
        rest = after.trim();
    }
    let (command, after) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    match command {
        "tools" => {
            let mut s = Session::open(server).await?;
            let r = s.request("tools/list", json!({})).await?;
            Ok(r["tools"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|t| format!("{}: {}", t["name"].as_str().unwrap_or(""), t["description"].as_str().unwrap_or("")))
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default())
        }
        "call" => {
            let after = after.trim();
            let (name, json_args) = after.split_once(char::is_whitespace).unwrap_or((after, ""));
            if name.is_empty() {
                return Err(Error::InvalidArgument("call needs a tool name, such as call list_talks {}".into()));
            }
            let args: Value = match json_args.trim() {
                "" => json!({}),
                j => serde_json::from_str(j).map_err(|e| Error::InvalidArgument(format!("arguments are not JSON: {e}")))?,
            };
            let mut s = Session::open(server).await?;
            let r = s.request("tools/call", json!({ "name": name, "arguments": args })).await?;
            let text = r["content"]
                .as_array()
                .map(|a| a.iter().filter_map(|c| c["text"].as_str()).collect::<Vec<_>>().join("\n"))
                .unwrap_or_default();
            if r["isError"].as_bool() == Some(true) { Err(Error::ToolFailed(text)) } else { Ok(text) }
        }
        "" => Err(Error::InvalidArgument("no command: use tools, or call <tool> <json>".into())),
        other => Err(Error::InvalidArgument(format!("unknown command {other}: use tools, or call <tool> <json>"))),
    }
}

struct Mcp;

impl Guest for Mcp {
    async fn run(args: RunArgs) -> Result<String, Error> {
        run(&args.arguments).await
    }
}

export!(Mcp);
