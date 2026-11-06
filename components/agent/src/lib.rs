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
//! the transcript as a timeline; curl gets the same text as ever. The page
//! asks for JSON (`Accept: application/json`) and so gets each tool result in
//! full, where the text transcript cuts each to one line of 300 characters.
//!
//! The harness itself holds no capabilities. Its world imports the model,
//! the agent SDK's `tools` (cosmonic:agent) and GitHub sign-in, and nothing
//! else from the host. `tools` is served by the toolbox host plugin, which lists the tool components in this
//! workload and calls the one asked for; whatever a tool can reach is decided
//! by that component's own manifest entry.
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
//! signed-in login. `tools.rs` turns the plugin's tool list into the
//! tool definitions the model sees, and the model's calls back into calls on
//! the plugin.

wit_bindgen::generate!({
    path: "../../wit",
    world: "workshop:toolbox/agent",
    generate_all,
});

#[path = "../../common/http.rs"]
mod http;
mod tools;

use exports::wasi::http::handler::Guest;
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
        // The chat page asks for JSON, which carries each tool result in full.
        // curl (and anything else) gets the plain-text transcript.
        let wants_json = req
            .get_headers()
            .get("accept")
            .iter()
            .any(|v| String::from_utf8_lossy(v).contains("application/json"));
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
        if prompt.is_empty() {
            let usage = "send a prompt as the request body, e.g. curl -d 'hello' ...\n";
            return http::text_response(200, usage.to_string());
        }
        let steps = run(&prompt).await;
        if wants_json {
            http::json_response(200, as_json(&steps))
        } else {
            http::text_response(200, as_text(&steps))
        }
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

/// One step of a run. The same steps are written two ways: as the plain-text
/// transcript curl gets (each tool result on one line, cut at 300
/// characters), or as JSON for the chat page, which shows each result in
/// full up to `MAX_PAGE_RESULT` bytes. The model reads every result whole
/// either way.
enum Step {
    User(String),
    Call { tool: String, args: String },
    Result { tool: String, ok: bool, text: String },
    Answer(String),
    ModelError(String),
    Note(String),
}

/// The most of one tool result the chat page gets. A diff from `gh pr diff`
/// is a few KB; anything past this is cut and marked as cut.
const MAX_PAGE_RESULT: usize = 64 * 1024;

/// The transcript as curl has always seen it: one line per step.
fn as_text(steps: &[Step]) -> String {
    let mut out = String::new();
    for step in steps {
        let line = match step {
            Step::User(p) => format!("[user] {p}"),
            Step::Call { tool, args } => format!("[model] call {tool}({args})"),
            Step::Result { tool, ok: true, text } => format!("[tool {tool}] ok: {}", one_line(text)),
            Step::Result { tool, ok: false, text } => {
                format!("[tool {tool}] error: {}", one_line(text))
            }
            Step::Answer(t) => format!("[model] {t}"),
            Step::ModelError(e) => format!("[model] error: {e}"),
            Step::Note(n) => format!("[agent] {n}"),
        };
        out.push_str(&line);
        out.push('\n');
    }
    out
}

/// The transcript for the chat page: `{"steps":[...]}`, each tool result in
/// full (up to `MAX_PAGE_RESULT` bytes, with `truncated` set past that).
fn as_json(steps: &[Step]) -> String {
    use serde_json::json;
    let steps: Vec<serde_json::Value> = steps
        .iter()
        .map(|step| match step {
            Step::User(p) => json!({ "kind": "user", "text": p }),
            Step::Call { tool, args } => json!({ "kind": "call", "tool": tool, "args": args }),
            Step::Result { tool, ok, text } => {
                let (shown, truncated) = clip(text, MAX_PAGE_RESULT);
                json!({
                    "kind": "result", "tool": tool, "ok": ok, "text": shown,
                    "truncated": truncated, "bytes": text.len(),
                })
            }
            Step::Answer(t) => json!({ "kind": "answer", "text": t }),
            Step::ModelError(e) => json!({ "kind": "model-error", "text": e }),
            Step::Note(n) => json!({ "kind": "note", "text": n }),
        })
        .collect();
    json!({ "steps": steps }).to_string()
}

/// At most `max` bytes of `s`, cut at a character boundary; true when cut.
fn clip(s: &str, max: usize) -> (&str, bool) {
    if s.len() <= max {
        return (s, false);
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    (&s[..end], true)
}

async fn run(prompt: &str) -> Vec<Step> {
    let mut out = Vec::new();
    // The model is offered exactly the tools this workload's manifest gives
    // the agent, as the toolbox plugin lists them.
    let offered = tools::list().await;
    let defs: Vec<ToolDef> = offered
        .iter()
        .map(|t| ToolDef { name: t.name.clone(), description: t.description.clone(), parameters: tools::schema(t) })
        .collect();

    let mut messages = vec![Message {
        role: "user".into(),
        content: prompt.into(),
        tool_name: None,
    }];
    out.push(Step::User(prompt.into()));

    for _ in 0..MAX_STEPS {
        match model::complete(messages.clone(), defs.clone()).await {
            Ok(Completion::Text(text)) => {
                out.push(Step::Answer(text));
                return out;
            }
            Ok(Completion::Call(call)) => {
                out.push(Step::Call { tool: call.name.clone(), args: call.arguments.clone() });
                let args: serde_json::Value =
                    serde_json::from_str(&call.arguments).unwrap_or_default();
                let result = match tools::call(out.len(), &call.name, &args).await {
                    Ok(ok) => {
                        out.push(Step::Result { tool: call.name.clone(), ok: true, text: ok.clone() });
                        ok
                    }
                    Err(err) => {
                        out.push(Step::Result { tool: call.name.clone(), ok: false, text: err.clone() });
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
                out.push(Step::ModelError(e.to_string()));
                return out;
            }
        }
    }
    out.push(Step::Note(format!("stopped after {MAX_STEPS} steps")));
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
