//! github-api: a host component plugin that reads GitHub for a workload, and
//! signs that workload in to GitHub when the person asks.
//!
//! The platform installs it on the host group (k8s/values.yaml), not a
//! workload. A workload reaches it only by declaring
//! `workshop:toolbox/github-api` and `github-auth` under `hostInterfaces`.
//! Two interfaces, two callers in step 4:
//!
//! * `github-auth`, which the agent harness imports: GitHub's OAuth device
//!   flow, done here from start to finish. The plugin asks GitHub for a code
//!   with no scopes, the person enters it on github.com, and the plugin polls
//!   until GitHub hands it a token. The token stays in this plugin's memory,
//!   keyed by the calling workload's id, which the host supplies
//!   (`wasmcloud:host/identity`). A caller only ever sees the code, the
//!   address to enter it at and the signed-in login. Nothing is written
//!   anywhere, so a host restart signs everyone out.
//! * `github-api`, which the gh tool imports: one GET to api.github.com,
//!   for the read paths the tool needs and nothing else, with the calling
//!   workload's token added when it has one. Everything else is refused, and
//!   the refusal names this plugin.
//!
//! Deny by default: a workload the host can't name gets nothing, a method
//! other than GET is refused, a path outside the allowlist is refused, and the
//! plugin builds every URL itself, so a caller supplies only a path. The
//! plugin's own allowedHosts (github.com, api.github.com) are the outer wall.

mod bindings {
    #![allow(unsafe_code)]
    wit_bindgen::generate!({ path: "../../wit", world: "workshop:toolbox/github-api-plugin", generate_all });
}

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use bindings::exports::workshop::toolbox::github_api::{self, Response, SendError};
use bindings::exports::workshop::toolbox::github_auth::{self, DevicePrompt, SignInStatus};
use bindings::wasi::http::types::{Fields, Method, Request, Response as HttpResponse, Scheme};
use bindings::wasmcloud::host::identity;
use serde_json::Value;

/// The API, for github-api. allowedHosts names it.
const API_HOST: &str = "api.github.com";
/// The device flow's two endpoints live here. allowedHosts names it.
const LOGIN_HOST: &str = "github.com";
const USER_AGENT: &str = "agent-tools-workshop-github-plugin (+https://github.com/cosmonic-labs/agent-tools-workshop)";
/// The config key for the OAuth app's client id, and the placeholder it holds
/// until the app exists. A client id is public; it is not a secret.
const CLIENT_ID_KEY: &str = "client-id";
const CLIENT_ID_PLACEHOLDER: &str = "REPLACE-WITH-GITHUB-OAUTH-APP-CLIENT-ID";
/// The most of one GitHub answer handed back; the gh tool asks for far less.
const MAX_BODY: usize = 4 * 1024 * 1024;

/// One workload's sign-in.
enum Session {
    /// Waiting for the person to enter the code.
    Pending {
        device_code: String,
        user_code: String,
        verification_uri: String,
        expires_at: Instant,
        interval: Duration,
        next_poll: Instant,
    },
    /// Signed in. The token never leaves this plugin.
    SignedIn { token: String, login: String },
}

thread_local! {
    /// Sign-ins by workload id. Memory only: lost when the host restarts.
    static SESSIONS: RefCell<HashMap<String, Session>> = RefCell::new(HashMap::new());
}

/// The calling workload's id, as the host resolves it, or a refusal.
fn caller(layer: &str) -> Result<String, String> {
    let id = identity::get_workload_id();
    if id.is_empty() {
        Err(format!(
            "{layer} plugin: the host could not say which workload is calling, so nothing is \
             sent and no token is used; refused"
        ))
    } else {
        Ok(id)
    }
}

fn token_for(workload: &str) -> Option<String> {
    SESSIONS.with(|s| match s.borrow().get(workload) {
        Some(Session::SignedIn { token, .. }) => Some(token.clone()),
        _ => None,
    })
}

struct Component;

// ---------------------------------------------------------------------------
// github-api: reads, for the gh tool.
// ---------------------------------------------------------------------------

impl github_api::Guest for Component {
    async fn send(method: String, path_and_query: String) -> Result<Response, SendError> {
        let workload = caller("github-api").map_err(SendError::Refused)?;
        if method != "GET" {
            return Err(SendError::Refused(format!(
                "github-api plugin: only GET requests are allowed; {method} {path_and_query} refused"
            )));
        }
        if let Err(why) = allowed(&path_and_query) {
            return Err(SendError::Refused(format!(
                "github-api plugin: {why}; GET {path_and_query} refused"
            )));
        }
        let mut headers = vec![
            ("accept", "application/vnd.github+json".to_string()),
            ("x-github-api-version", "2022-11-28".to_string()),
        ];
        if let Some(token) = token_for(&workload) {
            headers.push(("authorization", format!("Bearer {token}")));
        }
        let (status, all, body) = http(Method::Get, API_HOST, &path_and_query, &headers, Vec::new())
            .await
            .map_err(SendError::Failed)?;
        if body.len() > MAX_BODY {
            return Err(SendError::Failed(format!(
                "GitHub's answer was larger than {MAX_BODY} bytes; ask for fewer items"
            )));
        }
        // Only what a caller needs to pace itself.
        let headers = all
            .into_iter()
            .filter(|(k, _)| k.starts_with("x-ratelimit-") || k == "retry-after")
            .collect();
        Ok(Response { status, headers, body })
    }
}

/// Whether a path and query is one the gh tool needs: pull requests, their
/// files, reviews, review comments, conversation comments and checks, the
/// issue search, and the rate limit. Every segment must look like what it
/// stands for, so nothing can climb out of the shape.
fn allowed(path_and_query: &str) -> Result<(), String> {
    let (path, query) = path_and_query.split_once('?').unwrap_or((path_and_query, ""));
    let printable = |s: &str| s.bytes().all(|b| b.is_ascii_graphic() && b != b'#');
    if !path.starts_with('/') || !printable(path) || !printable(query) {
        return Err("the path must start with / and hold no spaces or fragments".into());
    }
    let owner = |s: &str| {
        !s.is_empty() && s.len() <= 39 && !s.starts_with('-') && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    };
    let repo = |s: &str| {
        !s.is_empty()
            && s.len() <= 100
            && s != "."
            && s != ".."
            && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    };
    let number = |s: &str| !s.is_empty() && s.len() <= 10 && s.bytes().all(|b| b.is_ascii_digit());
    // A commit sha or a percent-encoded branch name.
    let git_ref = |s: &str| {
        !s.is_empty()
            && s.len() <= 250
            && s != "."
            && s != ".."
            && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'%'))
    };
    let segments: Vec<&str> = path[1..].split('/').collect();
    let ok = match segments.as_slice() {
        ["rate_limit"] | ["search", "issues"] => true,
        ["repos", o, r, "pulls"] => owner(o) && repo(r),
        ["repos", o, r, "pulls", n] => owner(o) && repo(r) && number(n),
        ["repos", o, r, "pulls", n, "files" | "reviews" | "comments"] => {
            owner(o) && repo(r) && number(n)
        }
        ["repos", o, r, "issues", n, "comments"] => owner(o) && repo(r) && number(n),
        ["repos", o, r, "commits", g, "check-runs" | "status"] => owner(o) && repo(r) && git_ref(g),
        _ => false,
    };
    if ok {
        Ok(())
    } else {
        Err(format!(
            "{path} is not on this plugin's path allowlist (pull requests, their files, reviews, \
             comments and checks, the issue search, and the rate limit)"
        ))
    }
}

// ---------------------------------------------------------------------------
// github-auth: the device flow, for the agent harness.
// ---------------------------------------------------------------------------

impl github_auth::Guest for Component {
    async fn start() -> Result<DevicePrompt, String> {
        let workload = caller("github-auth")?;
        if let Some(login) = SESSIONS.with(|s| match s.borrow().get(&workload) {
            Some(Session::SignedIn { login, .. }) => Some(login.clone()),
            _ => None,
        }) {
            return Err(format!("github-auth plugin: already signed in as {login}; sign out first"));
        }
        let client_id = client_id()?;
        // No scope: the token reads public data, as an unauthenticated
        // request does, with a signed-in user's higher rate limit.
        let v = form(LOGIN_HOST, "/login/device/code", &[("client_id", &client_id)]).await?;
        if let Some(e) = v.get("error").and_then(Value::as_str) {
            return Err(format!("github-auth plugin: GitHub refused to start sign-in: {}", describe(&v, e)));
        }
        let text = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
        let num = |k: &str, d: u64| v.get(k).and_then(Value::as_u64).unwrap_or(d);
        let (Some(device_code), Some(user_code), Some(verification_uri)) =
            (text("device_code"), text("user_code"), text("verification_uri"))
        else {
            return Err("github-auth plugin: GitHub's answer to the sign-in request was incomplete".into());
        };
        let expires_in = num("expires_in", 900);
        let interval = Duration::from_secs(num("interval", 5));
        let now = Instant::now();
        let prompt = DevicePrompt {
            user_code: user_code.clone(),
            verification_uri: verification_uri.clone(),
            expires_in: expires_in as u32,
        };
        SESSIONS.with(|s| {
            s.borrow_mut().insert(
                workload,
                Session::Pending {
                    device_code,
                    user_code,
                    verification_uri,
                    expires_at: now + Duration::from_secs(expires_in),
                    interval,
                    next_poll: now + interval,
                },
            )
        });
        Ok(prompt)
    }

    async fn status() -> Result<SignInStatus, String> {
        let workload = caller("github-auth")?;
        let now = Instant::now();
        // Read what is needed, then let go of the map before any await.
        let pending = SESSIONS.with(|s| {
            let map = s.borrow();
            match map.get(&workload) {
                None => Err(SignInStatus::SignedOut),
                Some(Session::SignedIn { login, .. }) => Err(SignInStatus::SignedIn(login.clone())),
                Some(Session::Pending { device_code, user_code, verification_uri, expires_at, next_poll, .. }) => {
                    let prompt = DevicePrompt {
                        user_code: user_code.clone(),
                        verification_uri: verification_uri.clone(),
                        expires_in: expires_at.saturating_duration_since(now).as_secs() as u32,
                    };
                    Ok((device_code.clone(), *expires_at, *next_poll, prompt))
                }
            }
        });
        let (device_code, expires_at, next_poll, prompt) = match pending {
            Err(settled) => return Ok(settled),
            Ok(p) => p,
        };
        if now >= expires_at {
            forget(&workload);
            return Err("github-auth plugin: the code expired before it was entered; start again".into());
        }
        // GitHub asks to be polled no faster than its interval.
        if now < next_poll {
            return Ok(SignInStatus::Pending(prompt));
        }
        let client_id = client_id()?;
        let v = form(
            LOGIN_HOST,
            "/login/oauth/access_token",
            &[
                ("client_id", &client_id),
                ("device_code", &device_code),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ],
        )
        .await?;
        if let Some(token) = v.get("access_token").and_then(Value::as_str) {
            let login = login_of(token).await?;
            SESSIONS.with(|s| {
                s.borrow_mut().insert(
                    workload,
                    Session::SignedIn { token: token.to_string(), login: login.clone() },
                )
            });
            return Ok(SignInStatus::SignedIn(login));
        }
        match v.get("error").and_then(Value::as_str).unwrap_or("") {
            "authorization_pending" | "slow_down" => {
                let slow = v.get("interval").and_then(Value::as_u64);
                SESSIONS.with(|s| {
                    if let Some(Session::Pending { interval, next_poll, .. }) = s.borrow_mut().get_mut(&workload) {
                        if let Some(secs) = slow {
                            *interval = Duration::from_secs(secs);
                        }
                        *next_poll = Instant::now() + *interval;
                    }
                });
                Ok(SignInStatus::Pending(prompt))
            }
            e => {
                forget(&workload);
                Err(format!("github-auth plugin: sign-in ended: {}", describe(&v, e)))
            }
        }
    }

    async fn sign_out() -> Result<(), String> {
        let workload = caller("github-auth")?;
        forget(&workload);
        Ok(())
    }
}

fn forget(workload: &str) {
    SESSIONS.with(|s| s.borrow_mut().remove(workload));
}

/// The OAuth app's client id, from this plugin's config.
fn client_id() -> Result<String, String> {
    let id = bindings::wasi::config::store::get(CLIENT_ID_KEY)
        .map_err(|e| format!("github-auth plugin: could not read its config: {e:?}"))?
        .unwrap_or_default();
    let id = id.trim();
    if id.is_empty() || id == CLIENT_ID_PLACEHOLDER {
        return Err(format!(
            "github-auth plugin: GitHub sign-in is not set up on this cluster (no OAuth app: \
             the plugin's {CLIENT_ID_KEY} in k8s/values.yaml is unset). Everything works signed \
             out, at GitHub's 60 requests an hour."
        ));
    }
    Ok(id.to_string())
}

/// GitHub's error and its description, when it gave one.
fn describe(v: &Value, error: &str) -> String {
    match v.get("error_description").and_then(Value::as_str) {
        Some(d) => format!("{error} ({d})"),
        None if error.is_empty() => "no token and no error in GitHub's answer".into(),
        None => error.to_string(),
    }
}

/// The login a token belongs to, so the page can say who is signed in.
async fn login_of(token: &str) -> Result<String, String> {
    let headers = [
        ("accept", "application/vnd.github+json".to_string()),
        ("authorization", format!("Bearer {token}")),
    ];
    let (status, _, body) = http(Method::Get, API_HOST, "/user", &headers, Vec::new()).await?;
    let v: Value = serde_json::from_slice(&body).unwrap_or_default();
    match (status, v.get("login").and_then(Value::as_str)) {
        (200, Some(login)) => Ok(login.to_string()),
        _ => Err(format!("github-auth plugin: GitHub gave a token but would not say whose ({status})")),
    }
}

/// POST a form to a GitHub host and read its JSON answer.
async fn form(host: &str, path: &str, fields: &[(&str, &str)]) -> Result<Value, String> {
    let body: Vec<String> = fields.iter().map(|(k, v)| format!("{k}={}", enc(v))).collect();
    let headers = [
        ("accept", "application/json".to_string()),
        ("content-type", "application/x-www-form-urlencoded".to_string()),
    ];
    let (status, _, body) = http(Method::Post, host, path, &headers, body.join("&").into_bytes()).await?;
    serde_json::from_slice(&body)
        .map_err(|_| format!("github-auth plugin: {host}{path} answered {status} with something that is not JSON"))
}

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// One HTTPS request through wasi:http, which the plugin's allowedHosts limit.
/// Returns the status, the headers (lower case) and the body.
async fn http(
    method: Method,
    host: &str,
    path_and_query: &str,
    headers: &[(&str, String)],
    body: Vec<u8>,
) -> Result<(u16, Vec<(String, String)>, Vec<u8>), String> {
    let mut list = vec![("user-agent".to_string(), USER_AGENT.as_bytes().to_vec())];
    list.extend(headers.iter().map(|(k, v)| (k.to_string(), v.as_bytes().to_vec())));
    let fields = Fields::from_list(&list).map_err(|e| format!("headers: {e:?}"))?;
    let (mut body_tx, body_rx) = bindings::wit_stream::new();
    let (trailers_tx, trailers_rx) = bindings::wit_future::new(|| Ok(None));
    let (request, _sent) = Request::new(fields, Some(body_rx), trailers_rx, None);
    request.set_method(&method).map_err(|()| "bad method".to_string())?;
    request.set_scheme(Some(&Scheme::Https)).map_err(|()| "bad scheme".to_string())?;
    request.set_authority(Some(host)).map_err(|()| "bad authority".to_string())?;
    request
        .set_path_with_query(Some(path_and_query))
        .map_err(|()| format!("bad path: {path_and_query}"))?;
    wit_bindgen::spawn_local(async move {
        body_tx.write_all(body).await;
        drop(body_tx);
        let _ = trailers_tx.write(Ok(None)).await;
    });
    let response = bindings::wasi::http::client::send(request)
        .await
        .map_err(|e| format!("could not reach {host}: {e:?}"))?;
    let status = response.get_status_code();
    let headers = response
        .get_headers()
        .copy_all()
        .into_iter()
        .map(|(k, v)| (k.to_ascii_lowercase(), String::from_utf8_lossy(&v).into_owned()))
        .collect();
    let (_done_tx, done_rx) = bindings::wit_future::new(|| Ok(()));
    let (body, _trailers) = HttpResponse::consume_body(response, done_rx);
    Ok((status, headers, body.collect().await))
}

mod export {
    #![allow(unsafe_code)]
    use super::{Component, bindings};
    bindings::export!(Component with_types_in bindings);
}
