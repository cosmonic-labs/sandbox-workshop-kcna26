//! Demo websites for the steps, served by Host header. Agents reach them by
//! the Kubernetes Services in k8s/sites.yaml:
//!
//!   api.demo-sites.svc       GET  /data     an ordinary API page, the one host
//!                                           curl is granted
//!   api.demo-sites.svc       GET  /redirect a 302 to the attacker's server
//!   attacker.demo-sites.svc  ANY  /collect  a stand-in for an attacker's
//!                                           server: it answers anything sent
//!
//! The workload runs on its own host group (steps/00-demo-sites/demo-sites.yaml),
//! with local routing off, so a workload on another host group cannot answer
//! for these names. Admission policy decides who may use the demo-sites group.

wit_bindgen::generate!({
    path: "../../wit",
    world: "workshop:toolbox/demo-sites",
    generate_all,
});

#[path = "../../common/http.rs"]
mod http;

use exports::wasi::http::handler::Guest;
use wasi::http::types::{ErrorCode, Method, Request, Response};

/// The names this workload serves (host and host-aliases in its manifest).
const ATTACKER: &str = "attacker.demo-sites.svc";
/// The one host the curl tool is granted in the steps: an ordinary API the
/// agent is meant to call. `/data` is a normal page; `/redirect` sends the
/// caller to the attacker's server, a host curl is not granted, to show the
/// host checks each hop of a redirect.
const API: &str = "api.demo-sites.svc";

/// The normal page the api host serves, so a granted curl fetch returns
/// something ordinary.
const API_PAGE: &str = r#"{"service":"releases","version":"1.2","notes":"several bug fixes and performance improvements"}"#;

struct Sites;

impl Guest for Sites {
    async fn handle(req: Request) -> Result<Response, ErrorCode> {
        let host = req
            .get_headers()
            .get("host")
            .first()
            .map(|h| String::from_utf8_lossy(h).into_owned())
            .or_else(|| req.get_authority())
            .unwrap_or_default();
        let host = host.split(':').next().unwrap_or_default().to_string();
        let full = req.get_path_with_query().unwrap_or_default();
        let path = full.split_once('?').map_or(full.as_str(), |(p, _)| p).to_string();
        let method = match req.get_method() {
            Method::Get => "GET".to_string(),
            Method::Post => "POST".to_string(),
            other => format!("{other:?}").to_uppercase(),
        };
        let body = http::request_body(req).await;

        let (status, text) = match (host.as_str(), method.as_str(), path.as_str()) {
            (ATTACKER, _, "/collect") => match body.len() {
                0 => (200, "attacker: got your request\n".to_string()),
                n => (200, format!("attacker: got your request and {n} bytes of data\n")),
            },
            (API, "GET", "/data") => (200, API_PAGE.to_string()),
            (API, "GET", "/redirect") => {
                return http::redirect_response(302, "http://attacker.demo-sites.svc/collect");
            }
            _ => (404, format!("demo-sites: nothing at {method} {host}{path}\n")),
        };
        http::text_response(status, text)
    }
}

export!(Sites);
