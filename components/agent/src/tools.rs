//! The translation table: what a host does for you, written out by hand.
//!
//! A model only understands JSON. It is shown a list of tools, each with a
//! name, a description and a JSON Schema for its arguments, and it answers
//! with JSON such as {"name": "curl", "arguments": {"arguments": "-s
//! http://api.demo-sites.svc/data"}}. The tools only understand typed
//! functions such as `curl::run(RunArgs { arguments })`.
//!
//! Something has to translate between the two. A host could do it: read each
//! tool's WIT, build the definitions the model sees from the WIT's names,
//! types and doc comments, turn the model's JSON into a typed call, and turn
//! the typed result or error back into text. Upstream wasmCloud doesn't do
//! that yet, so the agent harness does it here, in plain sight: one row per
//! function, written to match `wit/toolbox.wit`.
//!
//! The rows follow the same rules a host follows:
//!
//! - Each function takes one `<name>-args` record. The record's fields are
//!   the tool's arguments: the field names are the JSON properties and the
//!   field doc comments are their descriptions.
//! - The function's doc comment is the tool's description.
//! - The tool's name is the WIT function name, or a name that reads better to
//!   a model (`curl` for the curl tool's `run`). A host keeps a rename table
//!   like that too.
//! - A typed error reaches the model as `{"case": "payload"}`, the JSON form
//!   of a WIT variant, so the model can tell a denied request from a network
//!   failure.
//!
//! Each of these three tools is a familiar command line program. Its one
//! argument is the command line you would type after the program's name, so a
//! tool learns new tricks without a new row here.

use crate::{curl, gh, sqlite};
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
    // workshop:toolbox/curl.run
    Tool {
        name: "curl",
        description: "Make an HTTP request with curl and return what it prints. Only the \
                      hosts this tool is granted can be reached.",
        params: &[(
            "arguments",
            "What you would type after curl, such as -s http://api.demo-sites.svc/data.",
        )],
    },
    // workshop:toolbox/sqlite.query
    Tool {
        name: "sqlite",
        description: "Run SQL against this tool's database and return the rows. Also accepts \
                      dot-commands such as .tables and .schema.",
        params: &[(
            "sql",
            "SQL to run, such as select title from talks limit 5, or a dot-command such as .tables.",
        )],
    },
    // workshop:toolbox/gh.run
    Tool {
        name: "gh",
        description: "Read pull requests on GitHub with gh and return what it prints: pr list, \
                      pr view, pr checks, pr diff, search prs. Only reads.",
        params: &[(
            "arguments",
            "What you would type after gh, such as pr list -R wasmCloud/wasmCloud.",
        )],
    },
];

/// Build the typed arguments from the model's JSON, make the typed call on
/// one tool slot, and turn the typed result back into text for the model.
/// An error comes back already rendered as `{"case": "payload"}`. Every tool
/// function is async (WASI 0.3), so each arm awaits the linked component.
pub async fn call(name: &str, a: &Args) -> Result<String, String> {
    match name {
        "curl" => {
            let args = curl::RunArgs { arguments: a.get("arguments")? };
            curl::run(args).await.map_err(curl_error)
        }
        "sqlite" => {
            let args = sqlite::QueryArgs { sql: a.get("sql")? };
            sqlite::query(args).await.map_err(sqlite_error)
        }
        "gh" => {
            let args = gh::RunArgs { arguments: a.get("arguments")? };
            gh::run(args).await.map_err(gh_error)
        }
        other => Err(variant("invalid-argument", Some(format!("no tool named '{other}'")))),
    }
}

/// A WIT variant case as JSON: `{"case": payload}`.
fn variant(case: &str, payload: Option<String>) -> String {
    let mut map = serde_json::Map::new();
    map.insert(case.to_string(), payload.map_or(Value::Null, Value::String));
    Value::Object(map).to_string()
}

fn curl_error(e: curl::Error) -> String {
    use curl::Error::*;
    match e {
        InvalidArgument(m) => variant("invalid-argument", Some(m)),
        AccessDenied(m) => variant("access-denied", Some(m)),
        Network(m) => variant("network", Some(m)),
        Other(m) => variant("other", m),
    }
}

fn sqlite_error(e: sqlite::Error) -> String {
    use sqlite::Error::*;
    match e {
        InvalidArgument(m) => variant("invalid-argument", Some(m)),
        AccessDenied(m) => variant("access-denied", Some(m)),
        NotFound(m) => variant("not-found", Some(m)),
        Other(m) => variant("other", m),
    }
}

fn gh_error(e: gh::Error) -> String {
    use gh::Error::*;
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
