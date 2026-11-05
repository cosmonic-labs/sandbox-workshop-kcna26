//! Toy sites for the steps, served by Host header. Agents reach them by the
//! Kubernetes Services in k8s/sites.yaml:
//!
//!   api.workshop-sites.svc        GET  /data     an ordinary API page, the one host
//!                                                curl is granted
//!   api.workshop-sites.svc        GET  /redirect a 302 to the collector
//!   collector.workshop-sites.svc  ANY  /collect  counts exfiltration attempts (keyvalue)
//!   sites.localhost  the api page and the collector too, for your laptop
//!                    through the cluster ingress
//!   any host         GET  /count    how many leaks arrived, and the last one
//!   sites.localhost  POST /reset    zero the counter
//!   sites.localhost  GET  /leaks    the last leaks as JSON, newest first
//!   sites.localhost  GET  /         a page that shows the leaks live, for a
//!                                   browser (curl /count says the same)
//!
//! Only sites.localhost, the name the laptop ingress uses, may reset or list
//! the leaks. A hijacked agent granted one of these hosts cannot wipe the
//! evidence by posting to its /reset.
//!
//! The workload runs on its own host group (steps/00-fixtures/toy-sites.yaml),
//! with local routing off, so a workload on another host group cannot answer
//! for these names. Admission policy decides who may use the fixtures group.

wit_bindgen::generate!({
    path: "../../wit",
    world: "workshop:toolbox/toy-sites",
    generate_all,
});

#[path = "../../common/http.rs"]
mod http;

use serde_json::{Value, json};
use exports::wasi::http::handler::Guest;
use wasi::http::types::{ErrorCode, Method, Request, Response};
use wasmcloud::keyvalue::{atomics, store};

/// The names this workload serves (host and host-aliases in its manifest).
const COLLECTOR: &str = "collector.workshop-sites.svc";
/// The one host the curl tool is granted in the steps: an ordinary API the
/// agent is meant to call. `/data` is a normal page; `/redirect` sends the
/// caller to the collector, a host curl is not granted, to show the host
/// checks each hop of a redirect.
const API: &str = "api.workshop-sites.svc";
const CONSOLE: &str = "sites.localhost";

/// The normal page the api host serves, so a granted curl fetch returns
/// something ordinary.
const API_PAGE: &str = r#"{"service":"releases","version":"1.2","notes":"several bug fixes and performance improvements"}"#;

async fn counter() -> Result<store::Bucket, String> {
    store::open("toy-collector".into()).await.map_err(|e| format!("{e:?}"))
}

fn kv_err<E: std::fmt::Debug>(e: E) -> String {
    format!("{e:?}")
}

/// The collector page, for a browser at http://sites.localhost:18180/.
const COLLECTOR_PAGE: &str = include_str!("collector.html");

/// How many leaks the page can list. Each one is kept under `leak-<n % RING>`
/// in the same bucket as the counter, so old ones are overwritten.
const RING: u64 = 50;
/// The most of one leak's body the page is shown.
const MAX_BODY: usize = 4000;

/// Who sent a leak, as far as the request tells: its Host and User-Agent.
struct Sender {
    host: String,
    agent: String,
}

async fn collect(body: &str, from: &Sender) -> Result<i64, String> {
    let b = counter().await?;
    let n = atomics::increment(&b, "leaks".into(), 1).await.map_err(kv_err)?;
    b.set("last".into(), body.as_bytes().to_vec(), None).await.map_err(kv_err)?;
    // The page's copy. A failure here must not change what the caller sees.
    let mut kept: String = body.chars().take(MAX_BODY).collect();
    if kept.len() < body.len() {
        kept.push_str("...");
    }
    let leak = json!({ "n": n, "host": from.host, "agent": from.agent,
                       "bytes": body.len(), "body": kept });
    let _ = b.set(ring_key(n as u64), leak.to_string().into_bytes(), None).await;
    Ok(n)
}

fn ring_key(n: u64) -> String {
    format!("leak-{}", n % RING)
}

async fn leak_count(b: &store::Bucket) -> Result<u64, String> {
    let n = b.get("leaks".into()).await.map_err(kv_err)?;
    // atomics stores the counter as decimal text on NATS; accept 8 BE bytes too.
    Ok(n.map(|v| {
        String::from_utf8_lossy(&v).trim().parse::<u64>().ok().unwrap_or_else(|| {
            <[u8; 8]>::try_from(v.as_slice()).map(u64::from_be_bytes).unwrap_or(0)
        })
    })
    .unwrap_or(0))
}

/// The last leaks, newest first, as JSON. With `?since=<n>` and no new leak
/// since, only the count, so a page polling every second stays cheap.
async fn leaks(since: Option<u64>) -> Result<String, String> {
    let b = counter().await?;
    let n = leak_count(&b).await?;
    if since == Some(n) {
        return Ok(json!({ "count": n, "unchanged": true }).to_string());
    }
    let mut list = Vec::new();
    for i in (n.saturating_sub(RING) + 1..=n).rev() {
        let Ok(Some(v)) = b.get(ring_key(i)).await else { continue };
        let Ok(leak) = serde_json::from_slice::<Value>(&v) else { continue };
        // Skip a slot left over from before a reset.
        if leak["n"].as_u64() == Some(i) {
            list.push(leak);
        }
    }
    Ok(json!({ "count": n, "leaks": list }).to_string())
}

async fn count() -> Result<String, String> {
    let b = counter().await?;
    let n = leak_count(&b).await?;
    let last = b.get("last".into()).await.map_err(kv_err)?;
    let last = last.map(|v| String::from_utf8_lossy(&v).into_owned()).unwrap_or_default();
    Ok(format!("leaks={n}\nlast={}\n", last.replace('\n', " | ")))
}

async fn reset() -> Result<(), String> {
    let b = counter().await?;
    b.delete("leaks".into()).await.map_err(kv_err)?;
    b.delete("last".into()).await.map_err(kv_err)?;
    for i in 0..RING {
        let _ = b.delete(format!("leak-{i}")).await;
    }
    Ok(())
}

struct Sites;

impl Guest for Sites {
    async fn handle(req: Request) -> Result<Response, ErrorCode> {
        let headers = req.get_headers();
        let host = headers
            .get("host")
            .first()
            .map(|h| String::from_utf8_lossy(h).into_owned())
            .or_else(|| req.get_authority())
            .unwrap_or_default();
        let host = host.split(':').next().unwrap_or_default().to_string();
        let full = req.get_path_with_query().unwrap_or_default();
        let (path, query) = full.split_once('?').unwrap_or((full.as_str(), ""));
        let (path, query) = (path.to_string(), query.to_string());
        let header = |name: &str| {
            headers.get(name).first().map(|h| String::from_utf8_lossy(h).into_owned()).unwrap_or_default()
        };
        let from = Sender { host: host.clone(), agent: header("user-agent") };
        let method = match req.get_method() {
            Method::Get => "GET".to_string(),
            Method::Post => "POST".to_string(),
            other => format!("{other:?}").to_uppercase(),
        };
        let body = String::from_utf8_lossy(&http::request_body(req).await).into_owned();

        let h = host.as_str();
        let (status, text) = match (h, method.as_str(), path.as_str()) {
            (COLLECTOR | CONSOLE, _, "/collect") => match collect(&body, &from).await {
                Ok(n) => (200, format!("collector: thanks, that is leak #{n}\n")),
                Err(e) => (500, e),
            },
            (API | CONSOLE, "GET", "/data") => (200, API_PAGE.to_string()),
            (API, "GET", "/redirect") => {
                return http::redirect_response(302, "http://collector.workshop-sites.svc/collect");
            }
            (_, "GET", "/count") => match count().await {
                Ok(t) => (200, t),
                Err(e) => (500, e),
            },
            (CONSOLE, "GET", "/") => return http::html_response(200, COLLECTOR_PAGE),
            (CONSOLE, "GET", "/leaks") => {
                let since = query.split('&').find_map(|kv| kv.strip_prefix("since=")?.parse().ok());
                match leaks(since).await {
                    Ok(t) => return http::json_response(200, t),
                    Err(e) => (500, e),
                }
            }
            (CONSOLE, "POST", "/reset") => match reset().await {
                Ok(()) => (200, "leaks=0\n".into()),
                Err(e) => (500, e),
            },
            _ => (404, format!("toy-sites: nothing at {method} {host}{path}\n")),
        };
        http::text_response(status, text)
    }
}

export!(Sites);

