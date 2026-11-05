// Vendored from cosmonic-labs/agent-tools (crates/github-core/src/lib.rs, commit aef5ffc),
// Apache-2.0, Copyright 2026 Cosmonic, Inc. See ../../LICENSE and ../../NOTICE.
// Modified: the tool definitions (defs.rs) are left out; `call` names the six tools itself.

//! The GitHub tool: read-only pull request triage on public repositories.
//!
//! Six tools (`gh_list_prs`, `gh_get_pr`, `gh_pr_files`, `gh_pr_checks`,
//! `gh_pr_comments`, `gh_search_prs`) over GitHub's REST API at
//! `api.github.com`, without credentials. HTTP is behind
//! `tool_common::http::Http`, which the component implements with
//! `wasi:http` and the tests with a fake, so all the logic here runs
//! natively.
//!
//! Unauthenticated requests are limited to 60 an hour per IP address (the
//! search API to 10 a minute). A refusal for the limit becomes a typed
//! `not-dispatched(upstream)` error that says when the limit resets, and
//! every result says how many requests are left.

mod client;
mod tools;
mod util;

use tool_common::http::Http;
use tool_common::{CallError, Reply};

pub use client::{RateLimit, API_HOST, USER_AGENT};
/// The six tools, by the names `call` takes.
pub const NAMES: &[&str] = &[
    "gh_list_prs",
    "gh_get_pr",
    "gh_pr_files",
    "gh_pr_checks",
    "gh_pr_comments",
    "gh_search_prs",
];

/// Run one call.
pub async fn call<H: Http>(http: &H, name: &str, arguments: &str) -> Result<Reply, CallError> {
    if !NAMES.contains(&name) {
        return Err(CallError::unknown_tool(name, NAMES));
    }
    let c = client::Client::new(http);
    match name {
        "gh_list_prs" => tools::list_prs(&c, arguments).await,
        "gh_get_pr" => tools::get_pr(&c, arguments).await,
        "gh_pr_files" => tools::pr_files(&c, arguments).await,
        "gh_pr_checks" => tools::pr_checks(&c, arguments).await,
        "gh_pr_comments" => tools::pr_comments(&c, arguments).await,
        "gh_search_prs" => tools::search_prs(&c, arguments).await,
        _ => unreachable!(),
    }
}
