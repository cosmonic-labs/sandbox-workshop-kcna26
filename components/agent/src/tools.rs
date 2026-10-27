//! The translation table: from the model's JSON tool calls to typed calls.
//!
//! A model only understands JSON. It is shown a list of tools, each with a
//! name, a description and a JSON Schema for its arguments, and it answers
//! with JSON such as {"name": "remember", "arguments": {"key": "...",
//! "fact": "..."}}. The tools only understand typed functions such as
//! `memory::remember(key, fact)`.
//!
//! Something has to translate between the two. In an agent platform the host
//! does this for you. Upstream wasmCloud has no host plugin for it, so the
//! agent harness does it here, in plain sight: one row per function the
//! model may call.

use crate::{memory, notes, reports, web};
use serde_json::{Value, json};

/// One function the model may call.
pub struct Tool {
    /// The name the model uses in its JSON tool call.
    pub name: &'static str,
    /// What the model is told the function does.
    pub description: &'static str,
    /// The argument names, in order, with a hint for the model. Every
    /// argument in this workshop is a string.
    pub params: &'static [(&'static str, &'static str)],
}

/// The whole table. `call` below makes the typed call behind each row.
pub const TOOLS: &[Tool] = &[
    Tool {
        name: "remember",
        description: "Remember a fact under a short key, for later conversations.",
        params: &[("key", "a short name for the fact"), ("fact", "the fact itself")],
    },
    Tool {
        name: "recall",
        description: "Look up the fact remembered under one key.",
        params: &[("key", "the key the fact was stored under")],
    },
    Tool {
        name: "recall_all",
        description: "List every fact remembered so far.",
        params: &[],
    },
    Tool {
        name: "read_note",
        description: "Read a file from the user's notes folder.",
        params: &[("path", "file name inside the notes folder, e.g. todo.txt")],
    },
    Tool {
        name: "append_note",
        description: "Add one line to the end of a file in the user's notes folder.",
        params: &[("path", "file name inside the notes folder"), ("line", "text to add")],
    },
    Tool {
        name: "save_report",
        description: "Save a text report to object storage.",
        params: &[("name", "report file name, e.g. report.txt"), ("body", "report text")],
    },
    Tool {
        name: "fetch",
        description: "Fetch a web page with HTTP GET and return its body.",
        params: &[("url", "the full URL, e.g. http://example.com/page")],
    },
    Tool {
        name: "post",
        description: "Send text to a URL with HTTP POST and return the response body.",
        params: &[("url", "the full URL"), ("body", "the text to send")],
    },
];

/// Pull the arguments out of the JSON, make the typed call on one tool slot,
/// and turn the typed result back into text for the model. Every tool
/// function is async (WASI 0.3), so each arm awaits the linked component.
pub async fn call(name: &str, a: &Args) -> Result<String, String> {
    match name {
        "remember" => {
            let (key, fact) = (a.get("key")?, a.get("fact")?);
            memory::remember(key.clone(), fact.clone()).await?;
            Ok(format!("remembered {key} = {fact}"))
        }
        "recall" => {
            let key = a.get("key")?;
            Ok(memory::recall(key.clone()).await?.unwrap_or(format!("nothing remembered under {key}")))
        }
        "recall_all" => {
            let entries = memory::recall_all().await?;
            if entries.is_empty() {
                return Ok("nothing remembered yet".into());
            }
            let lines: Vec<String> =
                entries.iter().map(|e| format!("{} = {}", e.key, e.fact)).collect();
            Ok(lines.join("; "))
        }
        "read_note" => notes::read(a.get("path")?).await,
        "append_note" => {
            let (path, line) = (a.get("path")?, a.get("line")?);
            notes::append(path.clone(), line).await?;
            Ok(format!("appended to {path}"))
        }
        "save_report" => {
            let (name, body) = (a.get("name")?, a.get("body")?);
            let n = body.len();
            reports::save(name.clone(), body).await?;
            Ok(format!("saved {n} bytes as {name}"))
        }
        "fetch" => web::fetch(a.get("url")?).await,
        "post" => web::post(a.get("url")?, a.get("body")?).await,
        other => Err(format!("no tool named '{other}'")),
    }
}

impl Tool {
    /// The JSON Schema the model sees for this function's arguments.
    pub fn schema(&self) -> String {
        let properties: serde_json::Map<String, Value> = self
            .params
            .iter()
            .map(|(name, hint)| (name.to_string(), json!({ "type": "string", "description": hint })))
            .collect();
        let required: Vec<&str> = self.params.iter().map(|(name, _)| *name).collect();
        json!({ "type": "object", "properties": properties, "required": required }).to_string()
    }
}

/// The model's JSON arguments, with one helper to read a string argument.
pub struct Args(pub Value);

impl Args {
    pub fn get(&self, name: &str) -> Result<String, String> {
        match self.0.get(name).and_then(Value::as_str) {
            Some(value) => Ok(value.to_string()),
            None => Err(format!("missing argument {name:?}")),
        }
    }
}
