//! not-installed: the placeholder behind every tool slot that has no real
//! tool yet.
//!
//! The agent harness imports one labelled slot per tool, and the host links
//! each label to the workload component of the same name. A slot with nothing
//! behind it would stop the workload from starting, so until a step installs
//! the real tool, the manifest fills the slot with this component. One image
//! exports all three tool interfaces, so the same image can sit behind any
//! slot. Every function returns its interface's `other` error saying the
//! tool isn't installed yet, which the harness shows in its transcript.

wit_bindgen::generate!({
    path: "../../wit",
    world: "workshop:toolbox/not-installed",
    generate_all,
});

use exports::workshop::toolbox::{memory, notes, web};

fn missing(tool: &str) -> Option<String> {
    Some(format!("the {tool} tool isn't installed yet"))
}

struct NotInstalled;

impl memory::Guest for NotInstalled {
    async fn remember(_: memory::RememberArgs) -> Result<(), memory::Error> {
        Err(memory::Error::Other(missing("memory")))
    }
    async fn recall(_: memory::RecallArgs) -> Result<Option<String>, memory::Error> {
        Err(memory::Error::Other(missing("memory")))
    }
    async fn recall_all() -> Result<Vec<memory::Entry>, memory::Error> {
        Err(memory::Error::Other(missing("memory")))
    }
}

impl notes::Guest for NotInstalled {
    async fn read(_: notes::ReadArgs) -> Result<String, notes::Error> {
        Err(notes::Error::Other(missing("notes")))
    }
    async fn append(_: notes::AppendArgs) -> Result<(), notes::Error> {
        Err(notes::Error::Other(missing("notes")))
    }
}

impl web::Guest for NotInstalled {
    async fn fetch(_: web::FetchArgs) -> Result<web::Response, web::Error> {
        Err(web::Error::Other(missing("web")))
    }
    async fn post(_: web::PostArgs) -> Result<web::Response, web::Error> {
        Err(web::Error::Other(missing("web")))
    }
}

export!(NotInstalled);
