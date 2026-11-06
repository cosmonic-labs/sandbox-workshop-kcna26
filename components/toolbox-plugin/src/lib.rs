//! toolbox (spike): a host component plugin that serves `tools` to a
//! workload by calling the tool components in that same workload.
//!
//! The plugin imports each tool interface it knows (curl, sqlite). The host
//! routes a call on one of those imports to the workload whose `tools` call
//! this task is serving, so a tool always runs in the caller's own workload,
//! with that tool's own grant. `list` reports which of the known tools the
//! caller's workload actually exports, from `workload-call.callable`.

mod bindings {
    #![allow(unsafe_code)]
    wit_bindgen::generate!({ path: "../../wit", world: "workshop:toolbox/toolbox-plugin", generate_all });
}

use bindings::exports::workshop::toolbox::tools::{self, Error, ToolInfo};
use bindings::wasmcloud::host::{identity, workload_call};
use bindings::workshop::toolbox::{curl, sqlite};

/// The tools this plugin knows: interface suffix, tool name, description, argument.
const KNOWN: &[(&str, &str, &str, &str)] = &[
    ("/curl", "curl", "HTTP requests from a command line, only to this tool's allowed hosts.", "arguments"),
    ("/sqlite", "sqlite", "SQL over the database files in this tool's folder.", "sql"),
];

struct Toolbox;

/// The interfaces the calling workload serves to this plugin.
fn caller_interfaces() -> Result<Vec<String>, Error> {
    let id = identity::get_workload_id();
    if id.is_empty() {
        return Err(Error::Other(Some("the host could not name the calling workload".into())));
    }
    Ok(workload_call::callable()
        .into_iter()
        .find(|(w, _)| *w == id)
        .map(|(_, ifaces)| ifaces)
        .unwrap_or_default())
}

impl tools::Guest for Toolbox {
    async fn list() -> Result<Vec<ToolInfo>, Error> {
        let ifaces = caller_interfaces()?;
        Ok(KNOWN
            .iter()
            .filter(|(suffix, ..)| ifaces.iter().any(|i| i.split('@').next().unwrap_or("").ends_with(suffix)))
            .map(|(_, name, description, argument)| ToolInfo {
                name: name.to_string(),
                description: description.to_string(),
                argument: argument.to_string(),
            })
            .collect())
    }

    async fn call(name: String, argument: String) -> Result<String, Error> {
        let have = Self::list().await?;
        if !have.iter().any(|t| t.name == name) {
            return Err(Error::NotFound(format!("no tool named {name} in this workload")));
        }
        match name.as_str() {
            "curl" => curl::run(curl::RunArgs { arguments: argument })
                .await
                .map_err(|e| Error::ToolError(format!("{e:?}"))),
            "sqlite" => sqlite::query(sqlite::QueryArgs { sql: argument })
                .await
                .map_err(|e| Error::ToolError(format!("{e:?}"))),
            _ => Err(Error::NotFound(name)),
        }
    }
}

bindings::export!(Toolbox with_types_in bindings);
