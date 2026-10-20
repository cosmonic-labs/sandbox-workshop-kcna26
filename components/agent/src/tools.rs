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
    /// Pull the arguments out of the JSON, make the typed call, and turn the
    /// typed result back into text for the model.
    pub call: fn(&Args) -> Result<String, String>,
}

/// The whole table. Each `call` is one typed function on one tool slot.
pub const TOOLS: &[Tool] = &[
    Tool {
        name: "remember",
        description: "Remember a fact under a short key, for later conversations.",
        params: &[("key", "a short name for the fact"), ("fact", "the fact itself")],
        call: |a| {
            let (key, fact) = (a.get("key")?, a.get("fact")?);
            memory::remember(&key, &fact)?;
            Ok(format!("remembered {key} = {fact}"))
        },
    },
    Tool {
        name: "recall",
        description: "Look up the fact remembered under one key.",
        params: &[("key", "the key the fact was stored under")],
        call: |a| {
            let key = a.get("key")?;
            Ok(memory::recall(&key)?.unwrap_or(format!("nothing remembered under {key}")))
        },
    },
    Tool {
        name: "recall_all",
        description: "List every fact remembered so far.",
        params: &[],
        call: |_| {
            let entries = memory::recall_all()?;
            if entries.is_empty() {
                return Ok("nothing remembered yet".into());
            }
            let lines: Vec<String> =
                entries.iter().map(|e| format!("{} = {}", e.key, e.fact)).collect();
            Ok(lines.join("; "))
        },
    },
    Tool {
        name: "read_note",
        description: "Read a file from the user's notes folder.",
        params: &[("path", "file name inside the notes folder, e.g. todo.txt")],
        call: |a| notes::read(&a.get("path")?),
    },
    Tool {
        name: "append_note",
        description: "Add one line to the end of a file in the user's notes folder.",
        params: &[("path", "file name inside the notes folder"), ("line", "text to add")],
        call: |a| {
            let (path, line) = (a.get("path")?, a.get("line")?);
            notes::append(&path, &line)?;
            Ok(format!("appended to {path}"))
        },
    },
    Tool {
        name: "save_report",
        description: "Save a text report to object storage.",
        params: &[("name", "report file name, e.g. report.txt"), ("body", "report text")],
        call: |a| {
            let (name, body) = (a.get("name")?, a.get("body")?);
            reports::save(&name, &body)?;
            Ok(format!("saved {} bytes as {name}", body.len()))
        },
    },
    Tool {
        name: "fetch",
        description: "Fetch a web page with HTTP GET and return its body.",
        params: &[("url", "the full URL, e.g. http://example.com/page")],
        call: |a| web::fetch(&a.get("url")?),
    },
    Tool {
        name: "post",
        description: "Send text to a URL with HTTP POST and return the response body.",
        params: &[("url", "the full URL"), ("body", "the text to send")],
        call: |a| web::post(&a.get("url")?, &a.get("body")?),
    },
];

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
