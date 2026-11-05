//! gh: read pull requests on GitHub, the way GitHub's own `gh` does.
//!
//! The tool holds no token and imports no HTTP client, no sockets and no
//! files. Its only way to GitHub is `workshop:toolbox/github-api`, served by
//! the github-api host component plugin, which sends each request to
//! api.github.com for the read paths it allows and adds the signed-in user's
//! token when there is one. The tool never sees the token.
//!
//! The requests, the status and rate-limit mapping and the output come from
//! github-core, vendored from cosmonic-labs/agent-tools (vendor/NOTICE).
//! `cli.rs` turns a gh command line into one of github-core's six tools; this
//! file gives github-core its HTTP through the capability.

mod bindings {
    #![allow(unsafe_code)]
    wit_bindgen::generate!({ path: "../../wit", world: "workshop:toolbox/gh-tool", generate_all });
}
mod cli;

use bindings::exports::workshop::toolbox::gh::{Error, Guest, RunArgs};
use bindings::workshop::toolbox::github_api as api;
use cli::{Command, Refusal};
use tool_common::http::{Http, Response, SendError};
use tool_common::{CallError, Dispatch, Outcome};

/// The most a `gh api` reply shows; the model reads all of what it gets.
const API_MAX: usize = 8 * 1024;

struct Component;

impl Guest for Component {
    async fn run(args: RunArgs) -> Result<String, Error> {
        match cli::parse(&args.arguments) {
            Ok(Command::Tool { name, arguments }) => {
                let reply = github_core::call(&ViaCapability, name, &arguments)
                    .await
                    .map_err(call_error)?;
                if reply.is_error {
                    Err(Error::Other(Some(reply.text)))
                } else {
                    Ok(reply.text)
                }
            }
            Ok(Command::Api { method, path }) => raw(method, path).await,
            Err(Refusal::Invalid(m)) => Err(Error::InvalidArgument(m)),
            Err(Refusal::Writes(m)) => Err(Error::AccessDenied(m)),
        }
    }
}

/// `gh api`: one request exactly as typed, through the capability, past this
/// tool's own commands. The plugin decides whether it is sent.
async fn raw(method: String, path: String) -> Result<String, Error> {
    match api::send(method, path).await {
        Ok(r) if (200..300).contains(&r.status) => {
            let body = String::from_utf8_lossy(&r.body);
            let shown = tool_common::bound::utf8_prefix(&body, API_MAX);
            if shown.len() < body.len() {
                Ok(format!("{shown}\n[{} more bytes not shown]", body.len() - shown.len()))
            } else {
                Ok(body.into_owned())
            }
        }
        Ok(r) => {
            let msg = serde_json::from_slice::<serde_json::Value>(&r.body)
                .ok()
                .and_then(|v| v.get("message")?.as_str().map(str::to_string))
                .unwrap_or_else(|| "no message".into());
            Err(Error::Other(Some(format!("gh: {msg} (HTTP {})", r.status))))
        }
        Err(api::SendError::Refused(m)) => Err(Error::AccessDenied(m)),
        Err(api::SendError::Failed(m)) => Err(Error::Network(m)),
    }
}

/// github-core's HTTP, over the github-api capability. The plugin fixes the
/// host (api.github.com) and the headers, so only the method and path cross.
struct ViaCapability;

impl Http for ViaCapability {
    async fn get(
        &self,
        _host: &str,
        path_and_query: &str,
        _headers: &[(&str, &str)],
    ) -> Result<Response, SendError> {
        match api::send("GET".to_string(), path_and_query.to_string()).await {
            Ok(r) => Ok(Response { status: r.status, headers: r.headers, body: r.body }),
            Err(api::SendError::Refused(m)) => Err(SendError::Denied(m)),
            Err(api::SendError::Failed(m)) => Err(SendError::NotSent(m)),
        }
    }

    fn now_unix(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }
}

/// github-core's error split, as this tool's WIT error.
fn call_error(e: CallError) -> Error {
    match e {
        CallError::NotDispatched(Dispatch::UnknownTool(m) | Dispatch::InvalidArguments(m)) => {
            Error::InvalidArgument(m)
        }
        CallError::NotDispatched(Dispatch::Denied(m)) => Error::AccessDenied(m),
        CallError::NotDispatched(Dispatch::Upstream(m))
        | CallError::OutcomeUnknown(Outcome::Upstream(m)) => Error::Network(m),
        CallError::NotDispatched(Dispatch::TimedOut) | CallError::OutcomeUnknown(Outcome::TimedOut) => {
            Error::Network("timed out".into())
        }
        CallError::NotDispatched(Dispatch::Other(m)) | CallError::OutcomeUnknown(Outcome::Other(m)) => {
            Error::Other(Some(m))
        }
        CallError::NotDispatched(Dispatch::Cancelled) | CallError::OutcomeUnknown(Outcome::Cancelled) => {
            Error::Other(Some("cancelled".into()))
        }
    }
}

mod export {
    #![allow(unsafe_code)]
    use super::{Component, bindings};
    bindings::export!(Component with_types_in bindings);
}
