//! github-api: a host component plugin that reads GitHub for a workload, and
//! signs that workload in to GitHub when the person asks.
//!
//! The platform installs it on the host group (k8s/values.yaml), not a
//! workload. A workload reaches it only by declaring
//! `workshop:toolbox/github-api` and `github-auth` under `hostInterfaces`.
//! Two interfaces, two callers in step 4:
//!
//! * `github-auth`, which the agent harness imports: GitHub sign-in, done
//!   here from start to finish, with no scopes. Two flows:
//!   - the browser flow, GitHub's OAuth web application flow with PKCE
//!     (S256). `begin-web` makes a state and a verifier, keeps them for the
//!     calling workload, and returns the github.com address to send the
//!     browser to. GitHub sends the browser back to the harness with a
//!     single-use code, and `complete-web` trades it for a token with the
//!     verifier and the OAuth app's client secret, neither of which ever
//!     leaves this plugin.
//!   - the code flow, GitHub's device flow (`start`, `status`): the person
//!     enters a code on github.com and the plugin polls until GitHub hands
//!     it a token. For Codespaces, where the browser can't come back to a
//!     fixed address.
//!
//!   Either way the token stays in this plugin's memory, keyed by the
//!   calling workload's id, which the host supplies
//!   (`wasmcloud:host/identity`). A caller sees at most a single-use code,
//!   the address to approve at and the signed-in login. Nothing is written
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
use bindings::exports::workshop::toolbox::github_auth::{self, DevicePrompt, SignInStatus, WebError};
use bindings::wasi::http::types::{Fields, Method, Request, Response as HttpResponse, Scheme};
use bindings::wasmcloud::host::identity;
use serde_json::Value;
use sha2::{Digest, Sha256};

/// The API, for github-api. allowedHosts names it.
const API_HOST: &str = "api.github.com";
/// Sign-in's endpoints (authorize, device code, access token) live here.
/// allowedHosts names it.
const LOGIN_HOST: &str = "github.com";
const USER_AGENT: &str = "agent-tools-workshop-github-plugin (+https://github.com/cosmonic-labs/agent-tools-workshop)";
/// The config key for the OAuth app's client id, and the placeholder it holds
/// until the app exists. A client id is public; it is not a secret.
const CLIENT_ID_KEY: &str = "client-id";
const CLIENT_ID_PLACEHOLDER: &str = "REPLACE-WITH-GITHUB-OAUTH-APP-CLIENT-ID";
/// The config key for the OAuth app's client secret, which the browser flow
/// needs to trade a code for a token, and the placeholder it holds until the
/// app's owner supplies it. It identifies the app; on its own it gets no
/// token (k8s/values.yaml says why it may sit in the repo).
const CLIENT_SECRET_KEY: &str = "client-secret";
const CLIENT_SECRET_PLACEHOLDER: &str = "REPLACE-WITH-GITHUB-OAUTH-APP-CLIENT-SECRET";
/// How long a browser sign-in may take, from begin-web to complete-web.
const WEB_TTL: Duration = Duration::from_secs(600);
/// The most browser sign-ins one workload may have waiting at once (one per
/// open tab, say). Beginning another drops the oldest.
const WEB_MAX_PENDING: usize = 5;
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

/// One browser sign-in that has begun and not yet completed.
struct WebPending {
    state: String,
    verifier: String,
    redirect_uri: String,
    expires_at: Instant,
}

thread_local! {
    /// Sign-ins by workload id. Memory only: lost when the host restarts.
    static SESSIONS: RefCell<HashMap<String, Session>> = RefCell::new(HashMap::new());
    /// Browser sign-ins waiting for GitHub to send the browser back, by
    /// workload id. Memory only, like SESSIONS.
    static WEB: RefCell<HashMap<String, Vec<WebPending>>> = RefCell::new(HashMap::new());
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

fn signed_in_as(workload: &str) -> Option<String> {
    SESSIONS.with(|s| match s.borrow().get(workload) {
        Some(Session::SignedIn { login, .. }) => Some(login.clone()),
        _ => None,
    })
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
        if let Some(login) = signed_in_as(&workload) {
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

    async fn begin_web(redirect_uri: String) -> Result<String, WebError> {
        let workload = caller("github-auth").map_err(WebError::Failed)?;
        if let Some(login) = signed_in_as(&workload) {
            return Err(WebError::Failed(format!(
                "github-auth plugin: already signed in as {login}; sign out first"
            )));
        }
        check_redirect_uri(&redirect_uri).map_err(WebError::Failed)?;
        let client_id = client_id().map_err(WebError::NotSetUp)?;
        // Without the secret the code could never be traded, so say so now,
        // before sending the person to GitHub for nothing.
        client_secret().map_err(WebError::NotSetUp)?;
        let state = random_token(32);
        let verifier = random_token(32);
        let url = authorize_url(&client_id, &redirect_uri, &state, &pkce_challenge(&verifier));
        let now = Instant::now();
        WEB.with(|w| {
            let mut map = w.borrow_mut();
            let list = map.entry(workload).or_default();
            remember(list, WebPending { state, verifier, redirect_uri, expires_at: now + WEB_TTL }, now);
        });
        Ok(url)
    }

    async fn complete_web(code: String, state: String) -> Result<String, WebError> {
        let workload = caller("github-auth").map_err(WebError::Failed)?;
        let now = Instant::now();
        // A state works once: take it out before anything else happens.
        let pending = WEB
            .with(|w| w.borrow_mut().get_mut(&workload).and_then(|list| take(list, &state, now)))
            .ok_or_else(|| {
                WebError::UnknownState(
                    "github-auth plugin: this sign-in link matches no sign-in this workload began, \
                     or it expired or was already used; sign in again"
                        .into(),
                )
            })?;
        if code.is_empty() || code.len() > 256 || !code.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(WebError::Failed("github-auth plugin: GitHub sent back no usable code".into()));
        }
        let client_id = client_id().map_err(WebError::NotSetUp)?;
        let client_secret = client_secret().map_err(WebError::NotSetUp)?;
        let v = form(
            LOGIN_HOST,
            "/login/oauth/access_token",
            &token_request(&client_id, &client_secret, &code, &pending.redirect_uri, &pending.verifier),
        )
        .await
        .map_err(WebError::Failed)?;
        let token = token_of(&v).map_err(WebError::Failed)?;
        let login = login_of(&token).await.map_err(WebError::Failed)?;
        SESSIONS.with(|s| {
            s.borrow_mut().insert(workload.clone(), Session::SignedIn { token, login: login.clone() })
        });
        WEB.with(|w| w.borrow_mut().remove(&workload));
        Ok(login)
    }
}

fn forget(workload: &str) {
    SESSIONS.with(|s| s.borrow_mut().remove(workload));
    WEB.with(|w| w.borrow_mut().remove(workload));
}

// ---------------------------------------------------------------------------
// The browser flow's pieces, kept free of host calls so they test natively.
// ---------------------------------------------------------------------------

/// A redirect address the plugin will hand GitHub: an absolute http or https
/// URL with a host, printable, no fragment, not too long. GitHub checks it
/// against the OAuth app's callback URL too.
fn check_redirect_uri(uri: &str) -> Result<(), String> {
    let rest = uri.strip_prefix("http://").or_else(|| uri.strip_prefix("https://"));
    let ok = match rest {
        Some(rest) => {
            let host = rest.split('/').next().unwrap_or("");
            !host.is_empty()
                && !host.contains('@')
                && uri.len() <= 512
                && uri.bytes().all(|b| b.is_ascii_graphic() && b != b'#')
        }
        None => false,
    };
    if ok {
        Ok(())
    } else {
        Err(format!("github-auth plugin: {uri:?} is not a usable redirect address (an http or https URL)"))
    }
}

/// GitHub's authorize address for one browser sign-in. No scope parameter:
/// the token gets no scopes.
fn authorize_url(client_id: &str, redirect_uri: &str, state: &str, challenge: &str) -> String {
    format!(
        "https://{LOGIN_HOST}/login/oauth/authorize?client_id={}&redirect_uri={}&state={}\
         &code_challenge={}&code_challenge_method=S256",
        enc(client_id),
        enc(redirect_uri),
        enc(state),
        enc(challenge)
    )
}

/// The PKCE S256 challenge for a verifier (RFC 7636): base64url, no padding,
/// of the verifier's SHA-256.
fn pkce_challenge(verifier: &str) -> String {
    b64url(&Sha256::digest(verifier.as_bytes()))
}

/// `n` random bytes from the host, base64url encoded: 32 bytes make the 43
/// characters RFC 7636 asks of a verifier at the least.
fn random_token(n: u64) -> String {
    b64url(&bindings::wasi::random::random::get_random_bytes(n))
}

fn b64url(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..=chunk.len() {
            out.push(ABC[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}

/// Keep one more waiting sign-in: drop the expired ones, and the oldest when
/// there are too many.
fn remember(list: &mut Vec<WebPending>, pending: WebPending, now: Instant) {
    list.retain(|p| p.expires_at > now);
    while list.len() >= WEB_MAX_PENDING {
        list.remove(0);
    }
    list.push(pending);
}

/// Take out the waiting sign-in with this state, if it has not expired.
fn take(list: &mut Vec<WebPending>, state: &str, now: Instant) -> Option<WebPending> {
    list.retain(|p| p.expires_at > now);
    let i = list.iter().position(|p| !state.is_empty() && p.state == state)?;
    Some(list.remove(i))
}

/// The form that trades a code for a token.
fn token_request<'a>(
    client_id: &'a str,
    client_secret: &'a str,
    code: &'a str,
    redirect_uri: &'a str,
    verifier: &'a str,
) -> [(&'static str, &'a str); 5] {
    [
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("code_verifier", verifier),
    ]
}

/// The token in GitHub's answer to the trade, or why there is none. GitHub
/// answers 200 with an `error` field when it refuses.
fn token_of(v: &Value) -> Result<String, String> {
    match v.get("access_token").and_then(Value::as_str) {
        Some(t) if !t.is_empty() => Ok(t.to_string()),
        _ => {
            let e = v.get("error").and_then(Value::as_str).unwrap_or("");
            Err(format!("github-auth plugin: GitHub would not trade the code for a token: {}", describe(v, e)))
        }
    }
}

/// The OAuth app's client id, from this plugin's config.
fn client_id() -> Result<String, String> {
    config(CLIENT_ID_KEY, CLIENT_ID_PLACEHOLDER)
}

/// The OAuth app's client secret, from this plugin's config. Only the
/// browser flow needs it.
fn client_secret() -> Result<String, String> {
    config(CLIENT_SECRET_KEY, CLIENT_SECRET_PLACEHOLDER)
}

fn config(key: &str, placeholder: &str) -> Result<String, String> {
    let value = bindings::wasi::config::store::get(key)
        .map_err(|e| format!("github-auth plugin: could not read its config: {e:?}"))?
        .unwrap_or_default();
    let value = value.trim();
    if value.is_empty() || value == placeholder {
        return Err(not_set_up(key));
    }
    Ok(value.to_string())
}

fn not_set_up(key: &str) -> String {
    format!(
        "github-auth plugin: GitHub sign-in is not set up on this cluster (the plugin's {key} \
         in k8s/values.yaml is unset). Everything works signed out, at GitHub's 60 requests an \
         hour."
    )
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

fn form_body(fields: &[(&str, &str)]) -> String {
    fields.iter().map(|(k, v)| format!("{k}={}", enc(v))).collect::<Vec<_>>().join("&")
}

/// POST a form to a GitHub host and read its JSON answer.
async fn form(host: &str, path: &str, fields: &[(&str, &str)]) -> Result<Value, String> {
    let headers = [
        ("accept", "application/json".to_string()),
        ("content-type", "application/x-www-form-urlencoded".to_string()),
    ];
    let (status, _, body) = http(Method::Post, host, path, &headers, form_body(fields).into_bytes()).await?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn b64url_matches_rfc_4648_without_padding() {
        assert_eq!(b64url(b""), "");
        assert_eq!(b64url(b"f"), "Zg");
        assert_eq!(b64url(b"fo"), "Zm8");
        assert_eq!(b64url(b"foo"), "Zm9v");
        assert_eq!(b64url(b"foob"), "Zm9vYg");
        assert_eq!(b64url(&[0xfb, 0xff, 0xbf]), "-_-_");
        assert_eq!(b64url(&[0u8; 32]).len(), 43);
    }

    #[test]
    fn pkce_challenge_is_rfc_7636_s256() {
        // RFC 7636, appendix B.
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn authorize_url_has_everything_but_a_scope() {
        let url = authorize_url("Ov23abc", "http://agent.localhost:18180/github/callback", "st-1", "ch_2");
        assert_eq!(
            url,
            "https://github.com/login/oauth/authorize?client_id=Ov23abc\
             &redirect_uri=http%3A%2F%2Fagent.localhost%3A18180%2Fgithub%2Fcallback&state=st-1\
             &code_challenge=ch_2&code_challenge_method=S256"
        );
        assert!(!url.contains("scope"));
    }

    #[test]
    fn redirect_uri_must_be_a_plain_http_url() {
        assert!(check_redirect_uri("http://agent.localhost:18180/github/callback").is_ok());
        assert!(check_redirect_uri("https://example.com/cb").is_ok());
        for bad in [
            "",
            "agent.localhost/github/callback",
            "javascript:alert(1)",
            "http:///nohost",
            "http://user@evil.example/cb",
            "http://agent.localhost/cb#frag",
            "http://agent.localhost/a b",
        ] {
            assert!(check_redirect_uri(bad).is_err(), "{bad} should be refused");
        }
    }

    fn pending(state: &str, expires_at: Instant) -> WebPending {
        WebPending {
            state: state.into(),
            verifier: format!("v-{state}"),
            redirect_uri: "http://agent.localhost:18180/github/callback".into(),
            expires_at,
        }
    }

    #[test]
    fn a_state_works_once() {
        let now = Instant::now();
        let mut list = Vec::new();
        remember(&mut list, pending("a", now + WEB_TTL), now);
        remember(&mut list, pending("b", now + WEB_TTL), now);
        assert_eq!(take(&mut list, "b", now).map(|p| p.verifier), Some("v-b".into()));
        assert!(take(&mut list, "b", now).is_none());
        assert!(take(&mut list, "nope", now).is_none());
        assert!(take(&mut list, "", now).is_none());
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn an_expired_state_is_refused_and_dropped() {
        let now = Instant::now();
        let mut list = Vec::new();
        remember(&mut list, pending("old", now + Duration::from_secs(1)), now);
        let later = now + Duration::from_secs(2);
        assert!(take(&mut list, "old", later).is_none());
        assert!(list.is_empty());
    }

    #[test]
    fn too_many_waiting_drops_the_oldest() {
        let now = Instant::now();
        let mut list = Vec::new();
        for i in 0..=WEB_MAX_PENDING {
            remember(&mut list, pending(&i.to_string(), now + WEB_TTL), now);
        }
        assert_eq!(list.len(), WEB_MAX_PENDING);
        assert!(take(&mut list, "0", now).is_none());
        assert!(take(&mut list, &WEB_MAX_PENDING.to_string(), now).is_some());
    }

    #[test]
    fn token_request_sends_secret_code_redirect_and_verifier() {
        let fields = token_request("id", "s3cr&t", "c0de", "http://a.localhost:1/cb", "ver");
        assert_eq!(
            form_body(&fields),
            "client_id=id&client_secret=s3cr%26t&code=c0de\
             &redirect_uri=http%3A%2F%2Fa.localhost%3A1%2Fcb&code_verifier=ver"
        );
    }

    #[test]
    fn token_answer_parsing() {
        let ok: Value = serde_json::from_str(r#"{"access_token":"gho_x","token_type":"bearer","scope":""}"#).unwrap();
        assert_eq!(token_of(&ok).unwrap(), "gho_x");
        let refused: Value = serde_json::from_str(
            r#"{"error":"bad_verification_code","error_description":"The code passed is incorrect or expired."}"#,
        )
        .unwrap();
        let e = token_of(&refused).unwrap_err();
        assert!(e.contains("bad_verification_code (The code passed is incorrect or expired.)"), "{e}");
        let empty: Value = serde_json::from_str(r#"{"access_token":""}"#).unwrap();
        assert!(token_of(&empty).unwrap_err().contains("no token and no error"));
    }

    #[test]
    fn not_set_up_says_which_key() {
        let e = not_set_up(CLIENT_SECRET_KEY);
        assert!(e.contains("GitHub sign-in is not set up on this cluster"));
        assert!(e.contains("client-secret"));
    }
}
