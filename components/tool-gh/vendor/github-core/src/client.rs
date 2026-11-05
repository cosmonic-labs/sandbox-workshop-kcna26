// Vendored from cosmonic-labs/agent-tools (crates/github-core/src/client.rs, commit aef5ffc),
// Apache-2.0, Copyright 2026 Cosmonic, Inc. See ../../LICENSE and ../../NOTICE.
// Modified for the workshop: a request may be signed in (the github-api plugin adds the
// credential, never this code), so the rate-limit and not-found messages no longer
// say the tool has no credentials, and the rate note says which limit applies.

//! Requests to GitHub and what their failures mean.

use std::cell::RefCell;

use serde_json::Value;
use tool_common::http::{send_error, Http, Response};
use tool_common::{CallError, Dispatch, Reply};

use crate::util::{until, utc};

/// The one host the tool reaches.
pub const API_HOST: &str = "api.github.com";
/// How the tool identifies itself to GitHub, which requires a User-Agent.
pub const USER_AGENT: &str = "cosmonic-github-tool/0.1 (+https://cosmonic.com)";
const API_VERSION: &str = "2022-11-28";

/// GitHub's rate limit as of the last response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateLimit {
    pub limit: u64,
    pub remaining: u64,
    /// Seconds since the epoch.
    pub reset: u64,
    /// `core` or `search`.
    pub resource: String,
}

/// How a call stops early.
pub enum Fail {
    /// Not dispatched, or outcome unknown.
    Call(CallError),
    /// GitHub answered with an error: the tool ran, changed nothing, and
    /// reports it as a completed error.
    Failed(String),
}

impl From<CallError> for Fail {
    fn from(e: CallError) -> Self {
        Fail::Call(e)
    }
}

pub struct Client<'h, H> {
    http: &'h H,
    rate: RefCell<Option<RateLimit>>,
    requests: RefCell<u32>,
}

fn rate_from(r: &Response) -> Option<RateLimit> {
    let num = |k: &str| r.header(k).and_then(|v| v.trim().parse::<u64>().ok());
    Some(RateLimit {
        limit: num("x-ratelimit-limit")?,
        remaining: num("x-ratelimit-remaining")?,
        reset: num("x-ratelimit-reset")?,
        resource: r
            .header("x-ratelimit-resource")
            .unwrap_or("core")
            .to_string(),
    })
}

fn message(body: &[u8]) -> String {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| {
            let mut m = v.get("message")?.as_str()?.to_string();
            if let Some(errs) = v.get("errors").and_then(|e| e.as_array()) {
                let details: Vec<String> = errs
                    .iter()
                    .filter_map(|e| {
                        e.get("message")
                            .and_then(|x| x.as_str())
                            .map(str::to_string)
                            .or_else(|| e.get("code").and_then(|x| x.as_str()).map(str::to_string))
                    })
                    .take(5)
                    .collect();
                if !details.is_empty() {
                    m.push_str(&format!(" ({})", details.join("; ")));
                }
            }
            Some(m)
        })
        .unwrap_or_else(|| String::from_utf8_lossy(&body[..body.len().min(300)]).into_owned())
}

impl<'h, H: Http> Client<'h, H> {
    pub fn new(http: &'h H) -> Self {
        Self {
            http,
            rate: RefCell::new(None),
            requests: RefCell::new(0),
        }
    }

    /// Requests made so far in this call.
    pub fn requests(&self) -> u32 {
        *self.requests.borrow()
    }

    pub fn rate(&self) -> Option<RateLimit> {
        self.rate.borrow().clone()
    }

    /// One line about the rate limit, for the end of a result.
    pub fn rate_note(&self) -> String {
        let n = self.requests();
        let used = format!(
            "{n} GitHub API request{} made",
            if n == 1 { "" } else { "s" }
        );
        match self.rate() {
            Some(r) => format!(
                "[{used}; {} of {} {} requests left until {} UTC ({})]",
                r.remaining,
                r.limit,
                r.resource,
                &utc(r.reset)[11..],
                if signed_in(&r) { "signed in" } else { "unauthenticated" }
            ),
            None => format!("[{used}]"),
        }
    }

    /// GET a JSON document. `what` names it for a not-found message.
    pub async fn get_json(&self, path_and_query: &str, what: &str) -> Result<Value, Fail> {
        *self.requests.borrow_mut() += 1;
        let headers = [
            ("accept", "application/vnd.github+json"),
            ("user-agent", USER_AGENT),
            ("x-github-api-version", API_VERSION),
        ];
        let resp = self
            .http
            .get(API_HOST, path_and_query, &headers)
            .await
            .map_err(|e| match send_error(API_HOST, e) {
                Ok(call) => Fail::Call(call),
                Err(m) => Fail::Failed(m),
            })?;
        if let Some(r) = rate_from(&resp) {
            *self.rate.borrow_mut() = Some(r);
        }
        match resp.status {
            200..=299 => serde_json::from_slice(&resp.body)
                .map_err(|e| Fail::Failed(format!("GitHub sent a response that is not JSON: {e}"))),
            403 | 429 if self.is_rate_limited(&resp) => {
                Err(CallError::NotDispatched(Dispatch::Upstream(self.rate_message(&resp))).into())
            }
            404 => Err(Fail::Failed(format!(
                "GitHub: {what} was not found. Either it does not exist, or it is private: this \
                 tool reads public repositories only."
            ))),
            401 => Err(Fail::Failed(format!(
                "GitHub refused the request as unauthorized: {}",
                message(&resp.body)
            ))),
            422 => Err(Fail::Failed(format!(
                "GitHub could not process the request: {}",
                message(&resp.body)
            ))),
            s @ 500..=599 => Err(Fail::Failed(format!(
                "GitHub had a server error ({s}): {}. Try again in a little while.",
                message(&resp.body)
            ))),
            s => Err(Fail::Failed(format!(
                "GitHub answered {s}: {}",
                message(&resp.body)
            ))),
        }
    }

    fn is_rate_limited(&self, r: &Response) -> bool {
        r.status == 429
            || r.header("x-ratelimit-remaining").map(str::trim) == Some("0")
            || r.header("retry-after").is_some()
            || message(&r.body).to_lowercase().contains("rate limit")
    }

    fn rate_message(&self, r: &Response) -> String {
        let now = self.http.now_unix();
        if let Some(secs) = r
            .header("retry-after")
            .and_then(|v| v.trim().parse::<u64>().ok())
        {
            return format!(
                "GitHub is limiting how fast this tool may send requests (a secondary rate \
                 limit). Wait {secs} seconds, until {} UTC, before calling a GitHub tool again.",
                &utc(now + secs)[11..]
            );
        }
        match rate_from(r) {
            Some(rl) if rl.remaining == 0 => {
                let auth = signed_in(&rl);
                let what = match (rl.resource == "search", auth) {
                    (true, _) => format!("{} searches a minute", rl.limit),
                    (false, true) => format!("{} requests an hour", rl.limit),
                    (false, false) => format!("{} requests an hour per IP address", rl.limit),
                };
                format!(
                    "GitHub's rate limit for {} {} requests is used up ({what}). It resets at {} \
                     UTC, {}. Wait until then before calling a GitHub tool again{}",
                    if auth { "signed-in" } else { "unauthenticated" },
                    rl.resource,
                    utc(rl.reset),
                    until(now, rl.reset),
                    if auth {
                        "."
                    } else {
                        ", or sign in with GitHub (the chat page's Sign in button) for a higher limit."
                    }
                )
            }
            _ => format!(
                "GitHub refused the request for its rate limit: {}. Wait a few minutes before \
                 calling a GitHub tool again.",
                message(&r.body)
            ),
        }
    }
}

/// Whether a rate limit is a signed-in one: GitHub gives unauthenticated
/// callers 60 core requests an hour and 10 searches a minute, and signed-in
/// users more of both.
fn signed_in(r: &RateLimit) -> bool {
    if r.resource == "search" {
        r.limit > 10
    } else {
        r.limit > 60
    }
}

pub fn finish(r: Result<Reply, Fail>) -> Result<Reply, CallError> {
    match r {
        Ok(reply) => Ok(reply),
        Err(Fail::Call(e)) => Err(e),
        Err(Fail::Failed(m)) => Ok(Reply::failed(m)),
    }
}
