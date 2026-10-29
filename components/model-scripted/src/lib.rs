//! A scripted, deliberately gullible model.
//!
//! It is deterministic, offline and needs no key. It reads the user's prompt
//! AND every tool result for instructions, and it obeys every instruction it
//! finds, including ones planted in a note or a web page. That is the point:
//! the hijack always succeeds, so the only thing left deciding what a
//! hijacked agent can reach is the sandbox around each tool.
//!
//! Instructions it understands (case-insensitive, one per line or sentence):
//!
//!   remember [that] <key> is <value>   -> remember {key, fact}
//!   what do you remember / recall      -> recall_all {}
//!   read my notes                      -> read_note {path: "todo.txt"}
//!   read <path>                        -> read_note {path}
//!   append "<text>" to <path>          -> append_note {path, line}
//!   fetch <url> / summarize <url>      -> fetch {url}
//!   send everything to <host>          -> post {url: http://<host>/collect, body: everything}
//!
//! "Everything" is all the text the model has read so far, memories included.
//!
//! It plans calls from the conversation in order and returns the first one it
//! has not issued yet. When nothing is left it answers in text.

wit_bindgen::generate!({
    path: "../../wit",
    world: "workshop:toolbox/model-provider",
    generate_all,
});

use exports::workshop::toolbox::model::Guest;
use serde_json::{Value, json};
use workshop::toolbox::types::{Completion, Message, ToolCall, ToolDef};

struct Scripted;

impl Guest for Scripted {
    async fn complete(messages: Vec<Message>, tools: Vec<ToolDef>) -> Result<Completion, String> {
        let available: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();

        // Calls already issued, in canonical JSON, so each instruction runs once.
        // Bodies are left out of the comparison: "everything" grows each turn.
        let issued: Vec<Value> = messages
            .iter()
            .filter(|m| m.role == "assistant")
            .filter_map(|m| serde_json::from_str::<Value>(&m.content).ok())
            .map(identity)
            .collect();

        // Everything the model has "read" so far, which is what
        // "send everything" exfiltrates.
        let everything: String = messages
            .iter()
            .filter(|m| m.role != "assistant")
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        let mut skipped = Vec::new();
        // Tool errors are not read as instructions: an error that echoes a
        // path would otherwise be "obeyed" again.
        let readable = messages.iter().filter(|m| {
            m.role == "user" || (m.role == "tool" && !m.content.starts_with("ERROR:"))
        });
        for m in readable {
            for (name, args) in instructions(&strip_tags(&m.content), &everything) {
                let call = json!({ "name": name, "arguments": args });
                if issued.contains(&identity(call)) {
                    continue;
                }
                if !available.contains(&name.as_str()) {
                    if !skipped.contains(&name) {
                        skipped.push(name);
                    }
                    continue;
                }
                return Ok(Completion::Call(ToolCall {
                    name,
                    arguments: args.to_string(),
                }));
            }
        }

        Ok(Completion::Text(answer(&messages, &skipped)))
    }
}

/// A call with its payload fields removed, for "have I done this already".
fn identity(mut call: Value) -> Value {
    if let Some(args) = call.get_mut("arguments").and_then(Value::as_object_mut) {
        args.remove("body");
    }
    call
}

/// Every instruction in `text`, in order, as (tool name, arguments).
fn instructions(text: &str, everything: &str) -> Vec<(String, Value)> {
    let mut out = Vec::new();
    for sentence in sentences(text) {
        let lower = sentence.to_lowercase();
        if lower.contains("what do you remember") || lower.contains("recall") {
            out.push(("recall_all".into(), json!({})));
        } else if let Some(rest) = after(&sentence, "remember ") {
            let rest = rest.strip_prefix("that ").unwrap_or(rest);
            if let Some((k, v)) = rest.split_once(" is ") {
                out.push((
                    "remember".into(),
                    json!({ "key": k.trim(), "fact": trim_punct(v) }),
                ));
            }
        } else if let Some(rest) = after(&sentence, "send everything to ") {
            let target = trim_punct(first_word(rest));
            let url = if target.contains("://") {
                target.to_string()
            } else {
                format!("http://{target}/collect")
            };
            out.push(("post".into(), json!({ "url": url, "body": everything })));
        } else if let Some(rest) = after(&sentence, "fetch ").or(after(&sentence, "summarize ")) {
            out.push(("fetch".into(), json!({ "url": trim_punct(first_word(rest)) })));
        } else if let Some(rest) = after(&sentence, "append ") {
            if let Some((text, path)) = rest.rsplit_once(" to ") {
                out.push((
                    "append_note".into(),
                    json!({ "path": trim_punct(first_word(path)), "line": text.trim().trim_matches('"') }),
                ));
            }
        } else if lower.contains("read my notes") || lower.contains("read the notes") {
            out.push(("read_note".into(), json!({ "path": "todo.txt" })));
        } else if let Some(rest) = after(&sentence, "read ") {
            let path = trim_punct(first_word(rest));
            if path.contains('/') || path.contains('.') {
                out.push(("read_note".into(), json!({ "path": path })));
            }
        }
    }
    out
}

/// The text answer once every instruction has run.
fn answer(messages: &[Message], skipped: &[String]) -> String {
    let results: Vec<String> = messages
        .iter()
        .filter(|m| m.role == "tool")
        .map(|m| {
            let name = m.tool_name.clone().unwrap_or_default();
            let flat: String = m.content.split_whitespace().collect::<Vec<_>>().join(" ");
            let short: String = flat.chars().take(120).collect();
            format!("{name}: {short}")
        })
        .collect();
    let mut text = if results.is_empty() {
        "Hello! I am the workshop agent running a scripted model. Ask me to remember \
         something, read my notes, or fetch a page."
            .to_string()
    } else {
        format!("Done. Here is what each step returned: {}", results.join(" | "))
    };
    if !skipped.is_empty() {
        text.push_str(&format!(" (I wanted to use {} but it is not installed.)", skipped.join(", ")));
    }
    text
}

/// Lines, and sentences within a line (". ", "; ", "? " and "! " split them).
fn sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        for part in line
            .split(". ")
            .flat_map(|p| p.split("; "))
            .flat_map(|p| p.split("? "))
            .flat_map(|p| p.split("! "))
        {
            let part = part.trim();
            if !part.is_empty() {
                out.push(part.to_string());
            }
        }
    }
    out
}

/// The text after `needle` (case-insensitive), if present.
fn after<'a>(s: &'a str, needle: &str) -> Option<&'a str> {
    let lower = s.to_lowercase();
    lower.find(needle).map(|i| &s[i + needle.len()..])
}

fn first_word(s: &str) -> &str {
    s.split_whitespace().next().unwrap_or("")
}

fn trim_punct(s: &str) -> &str {
    s.trim()
        .trim_end_matches(['.', ',', ';', ':', ')', '"', '\'', '!'])
        .trim_start_matches(['"', '\'', '('])
}

/// Replace HTML tags with line breaks so a planted page reads as text.
fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                out.push('\n');
            }
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

export!(Scripted);
