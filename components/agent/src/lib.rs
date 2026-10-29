//! The agent harness: the part of the agent that runs the loop.
//!
//! An agent here is four things working together: this harness, a model, a
//! set of tools and the storage those tools use. The harness answers HTTP:
//! send a prompt as the request body (or `?q=`) and it asks the model what to
//! do, runs the tool the model asks for, feeds the result back, and repeats
//! until the model answers in text. The response is the whole transcript so
//! you can watch every step. A browser that opens the root with no prompt
//! gets a page (`transcript.html`) that sends prompts the same way and draws
//! the transcript as a timeline; curl gets the same text as ever.
//!
//! The harness itself holds no capabilities. Its world imports the model and
//! three labelled tool slots and nothing else from the host; whatever a tool
//! can reach is decided by the manifest entry of the component filling that
//! slot. `tools.rs` translates the model's JSON into typed tool calls and
//! builds the tool definitions the model sees: work a host could do from the
//! WIT, done by hand here because upstream wasmCloud doesn't do it yet.

wit_bindgen::generate!({
    path: "../../wit",
    world: "workshop:toolbox/agent",
    generate_all,
});

#[path = "../../common/http.rs"]
mod http;
mod tools;

use exports::wasi::http::handler::Guest;
use tools::{Args, TOOLS};
use wasi::http::types::{ErrorCode, Method, Request, Response};
use workshop::toolbox::types::{Completion, Message, ToolDef};

/// Hard stop so a hijacked model cannot loop forever.
const MAX_STEPS: usize = 8;

/// The transcript page, for a browser at http://agent.localhost:18180/.
const PAGE: &str = include_str!("transcript.html");

struct Agent;

impl Guest for Agent {
    async fn handle(req: Request) -> Result<Response, ErrorCode> {
        let query = req
            .get_path_with_query()
            .and_then(|p| p.split_once('?').map(|(_, q)| q.to_string()))
            .unwrap_or_default();
        // A browser asks for HTML; curl sends */*. Only a browser gets the page.
        let wants_page = matches!(req.get_method(), Method::Get)
            && req
                .get_headers()
                .get("accept")
                .iter()
                .any(|v| String::from_utf8_lossy(v).contains("text/html"));
        let body = http::request_body(req).await;
        let mut prompt = String::from_utf8_lossy(&body).trim().to_string();
        if prompt.is_empty() {
            prompt = query
                .split('&')
                .find_map(|kv| kv.strip_prefix("q="))
                .map(url_decode)
                .unwrap_or_default();
        }
        if prompt.is_empty() && wants_page {
            return http::html_response(200, PAGE);
        }
        let transcript = if prompt.is_empty() {
            "send a prompt as the request body, e.g. curl -d 'hello' ...\n".to_string()
        } else {
            run(&prompt).await
        };
        http::text_response(200, transcript)
    }
}

export!(Agent);

async fn run(prompt: &str) -> String {
    let mut out = String::new();
    // Every function in the table is offered to the model. A slot that has
    // no real tool yet answers with a "not installed" error when called.
    let defs: Vec<ToolDef> = TOOLS
        .iter()
        .map(|t| ToolDef {
            name: t.name.into(),
            description: t.description.into(),
            parameters: t.schema(),
        })
        .collect();

    let mut messages = vec![Message {
        role: "user".into(),
        content: prompt.into(),
        tool_name: None,
    }];
    out.push_str(&format!("[user] {prompt}\n"));

    for _ in 0..MAX_STEPS {
        match model::complete(messages.clone(), defs.clone()).await {
            Ok(Completion::Text(text)) => {
                out.push_str(&format!("[model] {text}\n"));
                return out;
            }
            Ok(Completion::Call(call)) => {
                out.push_str(&format!("[model] call {}({})\n", call.name, call.arguments));
                if !TOOLS.iter().any(|t| t.name == call.name) {
                    out.push_str(&format!("[agent] no tool named '{}'\n", call.name));
                    return out;
                }
                let args: serde_json::Value =
                    serde_json::from_str(&call.arguments).unwrap_or_default();
                let result = match tools::call(&call.name, &Args(args.clone())).await {
                    Ok(ok) => {
                        out.push_str(&format!("[tool {}] ok: {}\n", call.name, one_line(&ok)));
                        ok
                    }
                    Err(err) => {
                        out.push_str(&format!("[tool {}] error: {}\n", call.name, one_line(&err)));
                        format!("ERROR: {err}")
                    }
                };
                messages.push(Message {
                    role: "assistant".into(),
                    content: serde_json::json!({ "name": call.name, "arguments": args })
                        .to_string(),
                    tool_name: None,
                });
                messages.push(Message {
                    role: "tool".into(),
                    content: result,
                    tool_name: Some(call.name.clone()),
                });
            }
            Err(e) => {
                out.push_str(&format!("[model] error: {e}\n"));
                return out;
            }
        }
    }
    out.push_str(&format!("[agent] stopped after {MAX_STEPS} steps\n"));
    out
}

fn one_line(s: &str) -> String {
    let flat: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > 300 {
        format!("{}...", flat.chars().take(300).collect::<String>())
    } else {
        flat
    }
}

fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                if let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    out.push(b);
                    i += 2;
                } else {
                    out.push(b'%');
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
