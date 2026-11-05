//! not-installed: the placeholder behind every tool slot that has no real
//! tool yet.
//!
//! The agent harness imports one labelled slot per tool, and the host links
//! each label to the workload component of the same name. A slot with nothing
//! behind it would stop the workload from starting, so until a step installs
//! the real tool, the manifest fills the slot with this component. One image
//! exports both the curl and the sqlite interfaces, so the same image can sit
//! behind either slot (gh has a placeholder of its own, gh-not-installed). Every function returns its interface's `other` error saying the
//! tool isn't installed yet, which the harness shows in its transcript.

wit_bindgen::generate!({
    path: "../../wit",
    world: "workshop:toolbox/not-installed",
    generate_all,
});

use exports::workshop::toolbox::{curl, sqlite};

fn missing(tool: &str) -> Option<String> {
    Some(format!("the {tool} tool isn't installed yet"))
}

struct NotInstalled;

impl curl::Guest for NotInstalled {
    async fn run(_: curl::RunArgs) -> Result<String, curl::Error> {
        Err(curl::Error::Other(missing("curl")))
    }
}

impl sqlite::Guest for NotInstalled {
    async fn query(_: sqlite::QueryArgs) -> Result<String, sqlite::Error> {
        Err(sqlite::Error::Other(missing("sqlite")))
    }
}

export!(NotInstalled);
