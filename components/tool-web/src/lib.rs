//! web: plain outbound HTTP requests.
//!
//! The only capability this component imports is wasi:http/outgoing-handler.
//! Which hosts it may reach is not in this code: it is the component's
//! `allowedHosts` in the manifest, enforced by the host on every request.

wit_bindgen::generate!({
    path: "../../wit",
    world: "workshop:toolbox/web-tool",
    generate_all,
});

use exports::workshop::toolbox::web::Guest;
use wstd::http::{Body, Client, Request};

/// Send one request and return the body of a successful response.
fn send(method: &str, url: &str, body: String) -> Result<String, String> {
    wstd::runtime::block_on(async move {
        let req = Request::builder()
            .method(method)
            .uri(url)
            .body(Body::from(body))
            .map_err(|e| format!("bad request: {e}"))?;
        let resp = Client::new()
            .send(req)
            .await
            .map_err(|e| format!("{method} {url} failed: {e:?}"))?;
        let status = resp.status();
        let mut body = resp.into_body();
        let bytes = body.contents().await.map_err(|e| e.to_string())?.to_vec();
        let text = String::from_utf8_lossy(&bytes).into_owned();
        if status.is_success() { Ok(text) } else { Err(format!("{method} {url}: HTTP {status}: {text}")) }
    })
}

struct Web;

impl Guest for Web {
    fn fetch(url: String) -> Result<String, String> {
        send("GET", &url, String::new())
    }

    fn post(url: String, body: String) -> Result<String, String> {
        send("POST", &url, body)
    }
}

export!(Web);
