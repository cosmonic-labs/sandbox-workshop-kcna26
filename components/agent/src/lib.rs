//! The workshop agent.
//!
//! It answers HTTP: send a prompt as the request body (or `?q=`) and it runs
//! a small loop: ask the model, run the tool the model asks for, feed the
//! result back, repeat until the model answers in text. The response is the
//! whole transcript so you can watch every step.
//!
//! The agent itself holds no capabilities. Its world imports the model and
//! four labelled tool slots and nothing else from the host; whatever a tool
//! can reach is decided by the manifest entry of the component filling that
//! slot.

wit_bindgen::generate!({
    path: "../../wit",
    world: "workshop:toolbox/agent",
    generate_all,
});

use workshop::toolbox::types::{Completion, Message, ToolDef};
use wstd::http::{Body, Request, Response};

/// Hard stop so a hijacked model cannot loop forever.
const MAX_STEPS: usize = 8;

type Describe = fn() -> ToolDef;
type Call = fn(&str) -> Result<String, String>;

/// The four labelled slots, in the order the agent offers them.
const SLOTS: [(&str, Describe, Call); 4] = [
    ("remember", remember::describe, remember::call),
    ("read-notes", read_notes::describe, read_notes::call),
    ("save-report", save_report::describe, save_report::call),
    ("fetch", fetch::describe, fetch::call),
];

#[wstd::http_server]
async fn main(mut req: Request<Body>) -> Result<Response<Body>, wstd::http::Error> {
    let query = req.uri().query().unwrap_or_default().to_string();
    let body = req.body_mut().contents().await?.to_vec();
    let mut prompt = String::from_utf8_lossy(&body).trim().to_string();
    if prompt.is_empty() {
        prompt = query
            .split('&')
            .find_map(|kv| kv.strip_prefix("q="))
            .map(url_decode)
            .unwrap_or_default();
    }
    let transcript = if prompt.is_empty() {
        "send a prompt as the request body, e.g. curl -d 'hello' ...\n".to_string()
    } else {
        run(&prompt)
    };
    Ok(Response::new(Body::from(transcript)))
}

fn run(prompt: &str) -> String {
    let mut out = String::new();
    // Ask every slot what it is; empty names are slots not installed yet.
    let mut installed: Vec<(ToolDef, Call)> = Vec::new();
    for (slot, describe, call) in SLOTS {
        let def = describe();
        if !def.name.is_empty() {
            out.push_str(&format!("[agent] slot {slot}: tool '{}' installed\n", def.name));
            installed.push((def, call));
        }
    }
    let defs: Vec<ToolDef> = installed.iter().map(|(d, _)| d.clone()).collect();

    let mut messages = vec![Message {
        role: "user".into(),
        content: prompt.into(),
        tool_name: None,
    }];
    out.push_str(&format!("[user] {prompt}\n"));

    for _ in 0..MAX_STEPS {
        match model::complete(&messages, &defs) {
            Ok(Completion::Text(text)) => {
                out.push_str(&format!("[model] {text}\n"));
                return out;
            }
            Ok(Completion::Call(call)) => {
                out.push_str(&format!("[model] call {}({})\n", call.name, call.arguments));
                let Some((_, run_tool)) = installed.iter().find(|(d, _)| d.name == call.name)
                else {
                    out.push_str(&format!("[agent] no tool named '{}'\n", call.name));
                    return out;
                };
                let result = match run_tool(&call.arguments) {
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
                    content: serde_json::json!({
                        "name": call.name,
                        "arguments": serde_json::from_str::<serde_json::Value>(&call.arguments)
                            .unwrap_or(serde_json::Value::String(call.arguments.clone())),
                    })
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
