//! toolbox: a host component plugin that serves the agent SDK's `tools`
//! interface (`cosmonic:agent/tools`) to a workload by calling the tool
//! components in that same workload. It is the host side of tool discovery:
//! an agent imports `tools` and gets whatever tools its manifest gives it.
//!
//! The platform installs it on the host group (k8s/values.yaml). A workload
//! reaches it by declaring `cosmonic:agent/tools` under `hostInterfaces`, and
//! offers it a tool by declaring that tool's interface (`curl`, `sqlite`,
//! `gh`) and running a component that exports it. The tools keep their own
//! typed interfaces; this plugin knows a fixed set of them.
//!
//! The plugin imports each tool interface it knows. The host routes a call
//! on one of those imports to the workload whose `tools` call this task is
//! serving, so a tool always runs in the caller's own workload, with that
//! tool's own grant. The plugin has no allowedHosts and no folders: it adds
//! no reach of its own, and a workload can never call another workload's
//! tools through it.
//!
//! It is also the translation table. A model only understands JSON. It is
//! shown a list of tools, each with a name, a description and a JSON Schema
//! for its arguments, and it answers with a name and a JSON object. The tools
//! only understand typed functions such as `curl::run(RunArgs { arguments })`.
//! Cosmonic Desktop and Control translate between the two for any typed tool,
//! reading the tool's WIT. This plugin knows three tools, so it is written out
//! by hand, one row per function, to match `wit/toolbox.wit`:
//!
//! - Each function takes one `<name>-args` record with one field, the tool's
//!   argument. The field's doc comment is what the model is told to put there.
//! - The function's doc comment is the tool's description.
//! - The tool's name reads better to a model than the function's (`curl` for
//!   the curl tool's `run`).
//! - A typed error is a completed call that failed: a `tool-response` with
//!   `is-error` set, its text `{"case": "payload"}`, the JSON form of a WIT
//!   variant, so the model can tell a denied request from a network failure.
//!   A tool this workload doesn't have was never dispatched:
//!   `not-dispatched(unknown-tool)`.
//!
//! Each tool is a familiar command line program, and its one argument is the
//! command line you would type after the program's name, so a tool learns
//! new tricks without a new row here.

mod bindings {
    #![allow(unsafe_code)]
    wit_bindgen::generate!({ path: "../../wit", world: "workshop:toolbox/toolbox-plugin", generate_all });
}

use bindings::cosmonic::agent::inference_types::{ToolArguments, ToolCall, ToolInput, ToolOutput};
use bindings::cosmonic::agent::tool_types::{CallError, DispatchError, ListError, ToolAnnotations, ToolDef, ToolResponse};
use bindings::exports::cosmonic::agent::tools;
use bindings::wasmcloud::host::{identity, workload_call};
use bindings::workshop::toolbox::{curl, gh, sqlite};
use serde_json::{Value, json};

/// One function a model may call.
struct Row {
    /// The tool's interface, as `callable` spells it without the version.
    interface: &'static str,
    /// The name the model uses.
    name: &'static str,
    /// The function's doc comment from the WIT.
    description: &'static str,
    /// The `<name>-args` record's one field, and its doc comment.
    argument: (&'static str, &'static str),
    /// (read-only, open-world): what the tool claims about itself, for display.
    hints: (bool, bool),
}

/// The whole table. `call` below makes the typed call behind each row.
const TABLE: &[Row] = &[
    // workshop:toolbox/curl.run
    Row {
        interface: "workshop:toolbox/curl",
        name: "curl",
        description: "Make an HTTP request with curl and return what it prints. Only the \
                      hosts this tool is granted can be reached.",
        argument: ("arguments", "What you would type after curl, such as -s http://api.mock-sites.svc/data."),
        hints: (false, true),
    },
    // workshop:toolbox/sqlite.query
    Row {
        interface: "workshop:toolbox/sqlite",
        name: "sqlite",
        description: "Run SQL against this tool's database and return the rows. Also accepts \
                      dot-commands such as .tables and .schema.",
        argument: ("sql", "SQL to run, such as select title from talks limit 5, or a dot-command such as .tables."),
        hints: (true, false),
    },
    // workshop:toolbox/gh.run
    Row {
        interface: "workshop:toolbox/gh",
        name: "gh",
        description: "Read pull requests on GitHub with gh and return what it prints: pr list, \
                      pr view, pr checks, pr diff, search prs. Only reads.",
        argument: ("arguments", "What you would type after gh, such as pr list -R wasmCloud/wasmCloud."),
        hints: (true, true),
    },
];

struct Toolbox;

/// The rows whose interface the calling workload serves to this plugin.
fn rows() -> Result<Vec<&'static Row>, String> {
    let id = identity::get_workload_id();
    if id.is_empty() {
        return Err("the host could not name the calling workload".into());
    }
    let served: Vec<String> = workload_call::callable()
        .into_iter()
        .find(|(w, _)| *w == id)
        .map(|(_, ifaces)| ifaces)
        .unwrap_or_default();
    Ok(TABLE
        .iter()
        .filter(|r| served.iter().any(|i| i.split('@').next() == Some(r.interface)))
        .collect())
}

impl Row {
    fn def(&self) -> ToolDef {
        let (field, doc) = self.argument;
        ToolDef {
            name: self.name.into(),
            title: None,
            description: self.description.into(),
            input: ToolInput::JsonSchema(
                json!({
                    "type": "object",
                    "properties": { field: { "type": "string", "description": doc } },
                    "required": [field],
                })
                .to_string(),
            ),
            output_schema: None,
            annotations: ToolAnnotations {
                read_only: Some(self.hints.0),
                destructive: Some(false),
                idempotent: None,
                open_world: Some(self.hints.1),
            },
            version: None,
        }
    }
}

impl tools::Guest for Toolbox {
    async fn list() -> Result<Vec<ToolDef>, ListError> {
        Ok(rows().map_err(ListError::Unavailable)?.into_iter().map(Row::def).collect())
    }

    async fn call(request: ToolCall) -> Result<ToolResponse, CallError> {
        let not_dispatched = |e| CallError::NotDispatched(e);
        let rows = rows().map_err(|m| not_dispatched(DispatchError::Upstream(m)))?;
        let Some(row) = rows.iter().find(|r| r.name == request.name) else {
            return Err(not_dispatched(DispatchError::UnknownTool(format!(
                "no tool named {} in this workload",
                request.name
            ))));
        };
        let argument = match &request.arguments {
            ToolArguments::Json(j) => serde_json::from_str::<Value>(j)
                .ok()
                .and_then(|v| v.get(row.argument.0).and_then(Value::as_str).map(String::from)),
            ToolArguments::Text(t) => Some(t.clone()),
        }
        .ok_or_else(|| {
            not_dispatched(DispatchError::InvalidArguments(format!("missing argument {:?}", row.argument.0)))
        })?;
        let result = match row.name {
            "curl" => curl::run(curl::RunArgs { arguments: argument }).await.map_err(curl_error),
            "sqlite" => sqlite::query(sqlite::QueryArgs { sql: argument }).await.map_err(sqlite_error),
            "gh" => gh::run(gh::RunArgs { arguments: argument }).await.map_err(gh_error),
            _ => unreachable!("every row has an arm"),
        };
        let (text, is_error) = match result {
            Ok(text) => (text, false),
            Err(json) => (json, true),
        };
        Ok(ToolResponse { output: vec![ToolOutput::Text(text)], structured: None, is_error })
    }
}

/// A tool's typed error as JSON: `{"case": payload}`.
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

bindings::export!(Toolbox with_types_in bindings);
