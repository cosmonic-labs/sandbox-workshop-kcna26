//! The translation table: what a host does for you, written out by hand.
//!
//! A model only understands JSON. It is shown a list of tools, each with a
//! name, a description and a JSON Schema for its arguments, and it answers
//! with JSON such as {"name": "remember", "arguments": {"key": "...",
//! "fact": "..."}}. The tools only understand typed functions such as
//! `memory::remember(RememberArgs { key, fact })`.
//!
//! Something has to translate between the two. On Cosmonic Desktop and
//! Cosmonic Control the host does it: it reads each tool's WIT, builds the
//! definitions the model sees from the WIT's names, types and doc comments,
//! turns the model's JSON into a typed call, and turns the typed result or
//! error back into text. Upstream wasmCloud has no host plugin for that yet,
//! so the agent harness does it here, in plain sight: one row per function,
//! written to match `wit/toolbox.wit`.
//!
//! The rows follow the same rules a host follows:
//!
//! - Each function takes one `<name>-args` record. The record's fields are
//!   the tool's arguments: the field names are the JSON properties and the
//!   field doc comments are their descriptions.
//! - The function's doc comment is the tool's description.
//! - The tool's name is the WIT function name in snake_case, or a name that
//!   reads better to a model (`read_note` for `notes.read`). A host keeps a
//!   rename table like that too.
//! - A typed error reaches the model as `{"case": "payload"}`, the JSON form
//!   of a WIT variant, so the model can tell a missing file from a denied
//!   one.
//! - A result with a `text` field (the web tool's `response`) shows its other
//!   fields as one compact JSON line, then the text.
//!
//! One shortcut: a host shows any other result as compact JSON. The rows
//! here write a short sentence instead, so the transcript reads well on a
//! slide.

use crate::{memory, notes, reports, web};
use serde_json::{Value, json};

/// One function the model may call.
pub struct Tool {
    /// The name the model uses in its JSON tool call.
    pub name: &'static str,
    /// The function's doc comment from the WIT: what the model is told the
    /// tool does.
    pub description: &'static str,
    /// The fields of the function's `<name>-args` record, in order, each
    /// with its doc comment from the WIT. Every field in this workshop is a
    /// required string.
    pub params: &'static [(&'static str, &'static str)],
}

/// The whole table. `call` below makes the typed call behind each row.
pub const TOOLS: &[Tool] = &[
    // workshop:toolbox/memory.remember
    Tool {
        name: "remember",
        description: "Remember a fact under a short key, for later conversations. A fact \
                      already stored under the key is replaced.",
        params: &[
            ("key", "A short name for the fact, such as \"favorite color\"."),
            ("fact", "The fact itself."),
        ],
    },
    // workshop:toolbox/memory.recall
    Tool {
        name: "recall",
        description: "Look up the fact remembered under one key. Returns none when nothing \
                      is stored under it.",
        params: &[("key", "The key the fact was stored under.")],
    },
    // workshop:toolbox/memory.recall-all
    Tool {
        name: "recall_all",
        description: "List every fact remembered so far, in no particular order.",
        params: &[],
    },
    // workshop:toolbox/notes.read
    Tool {
        name: "read_note",
        description: "Read a file from the user's notes folder and return its text.",
        params: &[("path", "The file's path inside the notes folder, such as todo.txt.")],
    },
    // workshop:toolbox/notes.append
    Tool {
        name: "append_note",
        description: "Add one line to the end of a file in the user's notes folder.",
        params: &[
            ("path", "The file's path inside the notes folder, such as todo.txt."),
            ("line", "The text to add. A line break is added after it."),
        ],
    },
    // workshop:toolbox/reports.save
    Tool {
        name: "save_report",
        description: "Save a text report to object storage. A report with the same name is \
                      replaced.",
        params: &[
            ("name", "The report's file name, such as report.txt."),
            ("body", "The report's text."),
        ],
    },
    // workshop:toolbox/web.fetch
    Tool {
        name: "fetch",
        description: "Fetch a web page with HTTP GET and return the server's answer.",
        params: &[("url", "The full URL, such as http://example.com/page.")],
    },
    // workshop:toolbox/web.post
    Tool {
        name: "post",
        description: "Send text to a URL with HTTP POST and return the server's answer.",
        params: &[
            ("url", "The full URL, such as http://example.com/form."),
            ("body", "The text to send."),
        ],
    },
];

/// Build the typed arguments from the model's JSON, make the typed call on
/// one tool slot, and turn the typed result back into text for the model.
/// An error comes back already rendered as `{"case": "payload"}`. Every tool
/// function is async (WASI 0.3), so each arm awaits the linked component.
pub async fn call(name: &str, a: &Args) -> Result<String, String> {
    match name {
        "remember" => {
            let args = memory::RememberArgs { key: a.get("key")?, fact: a.get("fact")? };
            let line = format!("remembered {} = {}", args.key, args.fact);
            memory::remember(args).await.map_err(memory_error)?;
            Ok(line)
        }
        "recall" => {
            let args = memory::RecallArgs { key: a.get("key")? };
            let key = args.key.clone();
            let fact = memory::recall(args).await.map_err(memory_error)?;
            Ok(fact.unwrap_or(format!("nothing remembered under {key}")))
        }
        "recall_all" => {
            let entries = memory::recall_all().await.map_err(memory_error)?;
            if entries.is_empty() {
                return Ok("nothing remembered yet".into());
            }
            let lines: Vec<String> =
                entries.iter().map(|e| format!("{} = {}", e.key, e.fact)).collect();
            Ok(lines.join("; "))
        }
        "read_note" => {
            let args = notes::ReadArgs { path: a.get("path")? };
            notes::read(args).await.map_err(notes_error)
        }
        "append_note" => {
            let args = notes::AppendArgs { path: a.get("path")?, line: a.get("line")? };
            let path = args.path.clone();
            notes::append(args).await.map_err(notes_error)?;
            Ok(format!("appended to {path}"))
        }
        "save_report" => {
            let args = reports::SaveArgs { name: a.get("name")?, body: a.get("body")? };
            let line = format!("saved {} bytes as {}", args.body.len(), args.name);
            reports::save(args).await.map_err(reports_error)?;
            Ok(line)
        }
        "fetch" => {
            let args = web::FetchArgs { url: a.get("url")? };
            web::fetch(args).await.map(response).map_err(web_error)
        }
        "post" => {
            let args = web::PostArgs { url: a.get("url")?, body: a.get("body")? };
            web::post(args).await.map(response).map_err(web_error)
        }
        other => Err(variant("invalid-argument", Some(format!("no tool named '{other}'")))),
    }
}

/// A result with a `text` field: the other fields as one JSON line, then the
/// text.
fn response(r: web::Response) -> String {
    format!("{}\n{}", json!({ "status": r.status }), r.text)
}

/// A WIT variant case as JSON: `{"case": payload}`.
fn variant(case: &str, payload: Option<String>) -> String {
    let mut map = serde_json::Map::new();
    map.insert(case.to_string(), payload.map_or(Value::Null, Value::String));
    Value::Object(map).to_string()
}

fn memory_error(e: memory::Error) -> String {
    use memory::Error::*;
    match e {
        InvalidArgument(m) => variant("invalid-argument", Some(m)),
        NoSuchStore(m) => variant("no-such-store", Some(m)),
        AccessDenied(m) => variant("access-denied", Some(m)),
        Unavailable(m) => variant("unavailable", Some(m)),
        Other(m) => variant("other", m),
    }
}

fn notes_error(e: notes::Error) -> String {
    use notes::Error::*;
    match e {
        InvalidArgument(m) => variant("invalid-argument", Some(m)),
        NotFound(m) => variant("not-found", Some(m)),
        AccessDenied(m) => variant("access-denied", Some(m)),
        Other(m) => variant("other", m),
    }
}

fn reports_error(e: reports::Error) -> String {
    use reports::Error::*;
    match e {
        InvalidArgument(m) => variant("invalid-argument", Some(m)),
        AccessDenied(m) => variant("access-denied", Some(m)),
        Unavailable(m) => variant("unavailable", Some(m)),
        Other(m) => variant("other", m),
    }
}

fn web_error(e: web::Error) -> String {
    use web::Error::*;
    match e {
        InvalidArgument(m) => variant("invalid-argument", Some(m)),
        AccessDenied(m) => variant("access-denied", Some(m)),
        Network(m) => variant("network", Some(m)),
        Other(m) => variant("other", m),
    }
}

impl Tool {
    /// The JSON Schema the model sees for this function's arguments: one
    /// property per record field, described by the field's doc comment.
    pub fn schema(&self) -> String {
        let properties: serde_json::Map<String, Value> = self
            .params
            .iter()
            .map(|(name, doc)| (name.to_string(), json!({ "type": "string", "description": doc })))
            .collect();
        let required: Vec<&str> = self.params.iter().map(|(name, _)| *name).collect();
        json!({ "type": "object", "properties": properties, "required": required }).to_string()
    }
}

/// The model's JSON arguments, with one helper to read a string field.
pub struct Args(pub Value);

impl Args {
    /// A required string field, or the invalid-argument error a host would
    /// give for a missing one.
    pub fn get(&self, name: &str) -> Result<String, String> {
        match self.0.get(name).and_then(Value::as_str) {
            Some(value) => Ok(value.to_string()),
            None => Err(variant("invalid-argument", Some(format!("missing argument {name:?}")))),
        }
    }
}
