//! not-installed: the placeholder behind every tool slot that has no real
//! tool yet.
//!
//! The agent harness imports one labelled slot per tool, and the host links
//! each label to the workload component of the same name. A slot with nothing
//! behind it would stop the workload from starting, so until a step installs
//! the real tool, the manifest fills the slot with this component. One image
//! exports all four tool interfaces, so the same image can sit behind any
//! slot. Every function returns the same plain error, which the harness
//! shows in its transcript.

wit_bindgen::generate!({
    path: "../../wit",
    world: "workshop:toolbox/not-installed",
    generate_all,
});

use exports::workshop::toolbox::{memory, notes, reports, web};

fn missing<T>(tool: &str) -> Result<T, String> {
    Err(format!("the {tool} tool isn't installed yet"))
}

struct NotInstalled;

impl memory::Guest for NotInstalled {
    async fn remember(_key: String, _fact: String) -> Result<(), String> {
        missing("memory")
    }
    async fn recall(_key: String) -> Result<Option<String>, String> {
        missing("memory")
    }
    async fn recall_all() -> Result<Vec<memory::Entry>, String> {
        missing("memory")
    }
}

impl notes::Guest for NotInstalled {
    async fn read(_path: String) -> Result<String, String> {
        missing("notes")
    }
    async fn append(_path: String, _line: String) -> Result<(), String> {
        missing("notes")
    }
}

impl reports::Guest for NotInstalled {
    async fn save(_name: String, _body: String) -> Result<(), String> {
        missing("reports")
    }
}

impl web::Guest for NotInstalled {
    async fn fetch(_url: String) -> Result<String, String> {
        missing("web")
    }
    async fn post(_url: String, _body: String) -> Result<String, String> {
        missing("web")
    }
}

export!(NotInstalled);
