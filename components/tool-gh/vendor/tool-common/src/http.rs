// Vendored from cosmonic-labs/agent-tools (crates/tool-common/src/http.rs, commit aef5ffc),
// Apache-2.0, Copyright 2026 Cosmonic, Inc. See ../../LICENSE and ../../NOTICE.
// Modified: the Http trait keeps only what github-core uses (no now_ms, sleep_ms).

//! Outbound HTTP GET, as the networked tools see it.
//!
//! A tool's logic is written against [`Http`], which the components
//! implement with `wasi:http/client` (the `tool-http` crate) and the tests
//! with canned responses. [`send_error`] is the one mapping from "no
//! response" to the call error split, so every networked tool says the same
//! thing about whether a request may have reached its server.

use std::future::Future;

use crate::types::{CallError, Dispatch, Outcome};

/// One HTTP response, body already read (and bounded by the implementer).
#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    /// Lower-case names.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Why a request produced no response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendError {
    /// Certainly not delivered (name resolution, connection refused, TLS).
    NotSent(String),
    /// The host's allow list refused it before sending.
    Denied(String),
    /// Timed out connecting: not delivered.
    ConnectTimeout,
    /// Delivered, or maybe delivered, and then failed or timed out.
    Sent { timed_out: bool, detail: String },
    /// The body was larger than the implementer's bound.
    TooLarge(usize),
}

/// GET requests over HTTPS.
pub trait Http {
    /// GET `https://{host}{path_and_query}` with `headers`.
    fn get(
        &self,
        host: &str,
        path_and_query: &str,
        headers: &[(&str, &str)],
    ) -> impl Future<Output = Result<Response, SendError>>;

    /// Seconds since the Unix epoch.
    fn now_unix(&self) -> u64;
}

/// What a request that produced no response means for the call: a
/// [`CallError`], or `Err(message)` for a completed failure (a body too
/// large, which reached the tool but could not be used).
pub fn send_error(host: &str, e: SendError) -> Result<CallError, String> {
    Ok(match e {
        SendError::NotSent(m) => {
            CallError::NotDispatched(Dispatch::Upstream(format!("could not reach {host}: {m}")))
        }
        SendError::Denied(m) => CallError::NotDispatched(Dispatch::Denied(m)),
        SendError::ConnectTimeout => CallError::NotDispatched(Dispatch::TimedOut),
        SendError::Sent {
            timed_out: true, ..
        } => CallError::OutcomeUnknown(Outcome::TimedOut),
        SendError::Sent { detail, .. } => CallError::OutcomeUnknown(Outcome::Upstream(format!(
            "the request to {host} failed after it was sent: {detail}. The tool only reads, so \
             a retry is safe."
        ))),
        SendError::TooLarge(n) => {
            return Err(format!(
                "{host}'s response was larger than {n} bytes; ask for fewer items"
            ))
        }
    })
}
