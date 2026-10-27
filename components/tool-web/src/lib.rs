//! web: plain outbound HTTP requests.
//!
//! The only capability this component imports is wasi:http/client (WASI
//! 0.3). Which hosts it may reach is not in this code: it is the component's
//! `allowedHosts` in the manifest, enforced by the host on every request.

wit_bindgen::generate!({
    path: "../../wit",
    world: "workshop:toolbox/web-tool",
    generate_all,
});

#[path = "../../common/http_client.rs"]
mod http_client;

use exports::workshop::toolbox::web::Guest;

/// Send one request and return the body of a successful response.
async fn send(method: &str, url: &str, body: String) -> Result<String, String> {
    let (status, bytes) = http_client::send(method, url, &[], body.into_bytes())
        .await
        .map_err(|e| format!("{method} {url} failed: {e}"))?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    if (200..300).contains(&status) { Ok(text) } else { Err(format!("{method} {url}: HTTP {status}: {text}")) }
}

struct Web;

impl Guest for Web {
    async fn fetch(url: String) -> Result<String, String> {
        send("GET", &url, String::new()).await
    }

    async fn post(url: String, body: String) -> Result<String, String> {
        send("POST", &url, body).await
    }
}

export!(Web);
