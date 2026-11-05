//! The agent harness: the part of the agent that runs the loop.
//!
//! An agent here is three things working together: this harness, a model and
//! a set of tools, each tool with only the grants its manifest entry gives
//! it. The harness answers HTTP:
//! send a prompt as the request body (or `?q=`) and it asks the model what to
//! do, runs the tool the model asks for, feeds the result back, and repeats
//! until the model answers in text. The response is the whole transcript so
//! you can watch every step. A browser that opens the root with no prompt
//! gets a page (`transcript.html`) that sends prompts the same way and draws
//! the transcript as a timeline; curl gets the same text as ever.
//!
//! The harness itself holds no capabilities. Its world imports the model,
//! three labelled tool slots (curl, sqlite and gh) and GitHub sign-in, and
//! nothing else from the host; whatever a tool can reach is
//! decided by the manifest entry of the component filling that slot.
//!
//! Sign-in is the one thing the harness does besides the loop, and it passes
//! every part of it to github-auth. The page's "Sign in with GitHub" opens
//! `/github/login`, which answers with a redirect to GitHub (the plugin's
//! browser flow); GitHub sends the browser back to `/github/callback` with a
//! single-use code, which the harness hands to the plugin. That code is
//! useless on its own: trading it for a token takes the plugin's PKCE
//! verifier and the OAuth app's client secret. `/github/sign-in` and
//! `/github/status` run the code flow (GitHub's device flow) for a terminal,
//! and `/github/sign-out` forgets the sign-in. The plugin keeps the token;
//! the harness sees at most the single-use code, a code to enter and the
//! signed-in login. `tools.rs` translates the model's JSON into
//! typed tool calls and builds the tool definitions the model sees: work a
//! host could do from the WIT, done by hand here because upstream wasmCloud
//! doesn't do it yet.

wit_bindgen::generate!({
    path: "../../wit",
    world: "workshop:toolbox/agent",
    generate_all,
});

#[path = "../../common/http.rs"]
mod http;
mod tools;

use exports::wasi::http::handler::Guest;
use tools::{Args, TOOLS};
use wasi::http::types::{ErrorCode, Method, Request, Response};
use workshop::toolbox::github_auth::{self, SignInStatus};
use workshop::toolbox::types::{Completion, Message, ToolDef};

/// Hard stop so a hijacked model cannot loop forever.
const MAX_STEPS: usize = 8;

/// The transcript page, for a browser at http://agent.localhost:18180/.
const PAGE: &str = include_str!("transcript.html");

/// Where GitHub sends the browser back after the person approves. It must
/// match the OAuth app's callback URL exactly. A request that names a port
/// in its Host header (as a browser does) gets that host and port instead,
/// so a cluster made with another HOST_HTTP port still sends a matching
/// address; the OAuth app's callback URL must then name that port too.
const DEFAULT_REDIRECT_URI: &str = "http://agent.localhost:18180/github/callback";

struct Agent;

impl Guest for Agent {
    async fn handle(req: Request) -> Result<Response, ErrorCode> {
        let query = req
            .get_path_with_query()
            .and_then(|p| p.split_once('?').map(|(_, q)| q.to_string()))
            .unwrap_or_default();
        // A browser asks for HTML; curl sends */*. Only a browser gets the page.
        let wants_page = matches!(req.get_method(), Method::Get)
            && req
                .get_headers()
                .get("accept")
                .iter()
                .any(|v| String::from_utf8_lossy(v).contains("text/html"));
        // A page on another site can make a browser POST here. Browsers name
        // that page in Origin; refuse when it is not this agent's own address.
        // curl sends no Origin, and the chat page's own POST matches Host.
        if matches!(req.get_method(), Method::Post) && !same_origin(&req) {
            return http::text_response(403, "cross-origin request refused\n".to_string());
        }
        let path = req
            .get_path_with_query()
            .map(|p| p.split('?').next().unwrap_or("").to_string())
            .unwrap_or_default();
        if matches!(req.get_method(), Method::Get) {
            match path.as_str() {
                "/github/login" => return github_login(&redirect_uri(&req)).await,
                "/github/callback" => return github_callback(&query).await,
                _ => {}
            }
        }
        if let Some(json) = github(&path, &req.get_method()).await {
            return http::json_response(200, json);
        }
        let body = http::request_body(req).await;
        let mut prompt = String::from_utf8_lossy(&body).trim().to_string();
        if prompt.is_empty() {
            prompt = query
                .split('&')
                .find_map(|kv| kv.strip_prefix("q="))
                .map(url_decode)
                .unwrap_or_default();
        }
        if prompt.is_empty() && wants_page {
            return http::html_response(200, PAGE);
        }
        let transcript = if prompt.is_empty() {
            "send a prompt as the request body, e.g. curl -d 'hello' ...\n".to_string()
        } else {
            run(&prompt).await
        };
        http::text_response(200, transcript)
    }
}

export!(Agent);

/// `GET /github/login`: begin the browser flow and send the browser to
/// GitHub. On failure, back to the page with a short reason it explains.
/// A plain navigation: it runs no tool, and the state the plugin makes is
/// what ties the callback to it.
async fn github_login(redirect_uri: &str) -> Result<Response, ErrorCode> {
    match github_auth::begin_web(redirect_uri.to_string()).await {
        Ok(url) => http::redirect_response(302, &url),
        Err(e) => back_to_page(&e),
    }
}

/// `GET /github/callback?code=..&state=..`: GitHub sending the browser back.
/// The plugin checks the state and trades the code; the harness never sees
/// the token. Then back to the page, which shows who is signed in, or why
/// not (`error=access_denied` when the person clicked Cancel on GitHub).
async fn github_callback(query: &str) -> Result<Response, ErrorCode> {
    let param = |name: &str| {
        query.split('&').find_map(|kv| {
            kv.split_once('=').filter(|(k, _)| *k == name).map(|(_, v)| url_decode(v))
        })
    };
    if let Some(error) = param("error") {
        let reason = if error == "access_denied" { "denied" } else { "failed" };
        return http::redirect_text(302, &format!("/?github={reason}"), format!("GitHub: {error}\n"));
    }
    let (code, state) = (param("code").unwrap_or_default(), param("state").unwrap_or_default());
    match github_auth::complete_web(code, state).await {
        Ok(_) => http::redirect_response(302, "/"),
        Err(e) => back_to_page(&e),
    }
}

/// A redirect to the page with one of a few fixed reasons, which the page
/// turns into its own words. The page never shows text from the address, so
/// a crafted link can't put words on it. The body has the plugin's message,
/// for curl.
fn back_to_page(e: &github_auth::WebError) -> Result<Response, ErrorCode> {
    use github_auth::WebError;
    let (reason, message) = match e {
        WebError::NotSetUp(m) => ("not-set-up", m),
        WebError::UnknownState(m) => ("expired", m),
        WebError::Failed(m) => ("failed", m),
    };
    http::redirect_text(302, &format!("/?github={reason}"), format!("{message}\n"))
}

/// The browser flow's redirect address: the request's own host and port
/// when its Host header names a port, else the default.
fn redirect_uri(req: &Request) -> String {
    let host = req
        .get_headers()
        .get("host")
        .first()
        .map(|v| String::from_utf8_lossy(v).trim().to_ascii_lowercase())
        .unwrap_or_default();
    let plain = host.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':'));
    match host.split_once(':') {
        Some((name, port))
            if plain && !name.is_empty() && !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) =>
        {
            format!("http://{host}/github/callback")
        }
        _ => DEFAULT_REDIRECT_URI.to_string(),
    }
}

/// The code flow and sign-out, passed to github-auth. The answer is
/// JSON for the page: `state` is signed-out, pending (with the code to enter
/// and where), signed-in (with the login) or error (with why). None of it is
/// the token: github-auth never hands that out.
async fn github(path: &str, method: &Method) -> Option<String> {
    use serde_json::json;
    let result = match (path, method) {
        ("/github/status", Method::Get) => github_auth::status().await,
        ("/github/sign-in", Method::Post) => github_auth::start().await.map(SignInStatus::Pending),
        ("/github/sign-out", Method::Post) => {
            github_auth::sign_out().await.map(|()| SignInStatus::SignedOut)
        }
        _ => return None,
    };
    let v = match result {
        Ok(SignInStatus::SignedOut) => json!({ "state": "signed-out" }),
        Ok(SignInStatus::Pending(p)) => json!({
            "state": "pending",
            "user_code": p.user_code,
            "verification_uri": p.verification_uri,
            "expires_in": p.expires_in,
        }),
        Ok(SignInStatus::SignedIn(login)) => json!({ "state": "signed-in", "login": login }),
        Err(e) => json!({ "state": "error", "error": e }),
    };
    Some(v.to_string())
}

async fn run(prompt: &str) -> String {
    let mut out = String::new();
    // Every function in the table is offered to the model. A slot that has
    // no real tool yet answers with a "not installed" error when called.
    let defs: Vec<ToolDef> = TOOLS
        .iter()
        .map(|t| ToolDef {
            name: t.name.into(),
            description: t.description.into(),
            parameters: t.schema(),
        })
        .collect();

    let mut messages = vec![Message {
        role: "user".into(),
        content: prompt.into(),
        tool_name: None,
    }];
    out.push_str(&format!("[user] {prompt}\n"));

    for _ in 0..MAX_STEPS {
        match model::complete(messages.clone(), defs.clone()).await {
            Ok(Completion::Text(text)) => {
                out.push_str(&format!("[model] {text}\n"));
                return out;
            }
            Ok(Completion::Call(call)) => {
                out.push_str(&format!("[model] call {}({})\n", call.name, call.arguments));
                if !TOOLS.iter().any(|t| t.name == call.name) {
                    out.push_str(&format!("[agent] no tool named '{}'\n", call.name));
                    return out;
                }
                let args: serde_json::Value =
                    serde_json::from_str(&call.arguments).unwrap_or_default();
                let result = match tools::call(&call.name, &Args(args.clone())).await {
                    Ok(ok) => {
                        out.push_str(&format!("[tool {}] ok: {}\n", call.name, one_line(&ok)));
                        ok
                    }
                    Err(err) => {
                        out.push_str(&format!("[tool {}] error: {}\n", call.name, one_line(&err)));
                        format!("ERROR: {err}")
                    }
                };
                messages.push(Message {
                    role: "assistant".into(),
                    content: serde_json::json!({ "name": call.name, "arguments": args })
                        .to_string(),
                    tool_name: None,
                });
                messages.push(Message {
                    role: "tool".into(),
                    content: result,
                    tool_name: Some(call.name.clone()),
                });
            }
            Err(e) => {
                out.push_str(&format!("[model] error: {e}\n"));
                return out;
            }
        }
    }
    out.push_str(&format!("[agent] stopped after {MAX_STEPS} steps\n"));
    out
}

/// True when the request has no Origin header, or its Origin names the same
/// host and port the request was sent to (the Host header).
fn same_origin(req: &Request) -> bool {
    let headers = req.get_headers();
    let first = |name: &str| {
        headers.get(name).first().map(|v| String::from_utf8_lossy(v).trim().to_ascii_lowercase())
    };
    let Some(origin) = first("origin") else { return true };
    let host = first("host").or_else(|| req.get_authority().map(|a| a.to_ascii_lowercase()));
    let origin_host = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
        .unwrap_or("");
    matches!(host, Some(h) if !h.is_empty() && h == origin_host)
}

fn one_line(s: &str) -> String {
    let flat: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > 300 {
        format!("{}...", flat.chars().take(300).collect::<String>())
    } else {
        flat
    }
}

fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                if let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    out.push(b);
                    i += 2;
                } else {
                    out.push(b'%');
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
