//! fetch: makes one outbound HTTP request.
//!
//! The only capability this component imports is wasi:http/outgoing-handler.
//! Which hosts it may reach is not in this code: it is the component's
//! `allowedHosts` in the manifest, enforced by the host on every request.

wit_bindgen::generate!({
    path: "../../wit",
    world: "workshop:toolbox/tool-provider",
    generate_all,
});

use exports::workshop::toolbox::tool::Guest;
use serde_json::Value;
use workshop::toolbox::types::ToolDef;
use wstd::http::{Body, Client, Request};

struct Fetch;

impl Guest for Fetch {
    fn describe() -> ToolDef {
        ToolDef {
            name: "fetch".into(),
            description: "Fetch a URL over HTTP (GET, or POST with a body).".into(),
            parameters: r#"{"type":"object","properties":{"url":{"type":"string"},"method":{"enum":["GET","POST"]},"body":{"type":"string"}},"required":["url"]}"#.into(),
        }
    }

    fn call(arguments: String) -> Result<String, String> {
        let args: Value = serde_json::from_str(&arguments).map_err(|e| e.to_string())?;
        let url = args.get("url").and_then(Value::as_str).unwrap_or_default().to_string();
        let method = args.get("method").and_then(Value::as_str).unwrap_or("GET").to_string();
        let body = args.get("body").and_then(Value::as_str).unwrap_or_default().to_string();
        wstd::runtime::block_on(async move {
            let req = Request::builder()
                .method(method.as_str())
                .uri(&url)
                .body(Body::from(body))
                .map_err(|e| format!("bad request: {e}"))?;
            let resp = Client::new()
                .send(req)
                .await
                .map_err(|e| format!("{method} {url} failed: {e:?}"))?;
            let status = resp.status();
            let mut body = resp.into_body();
            let bytes = body.contents().await.map_err(|e| e.to_string())?;
            Ok(format!("HTTP {status}\n{}", String::from_utf8_lossy(bytes)))
        })
    }
}

export!(Fetch);
