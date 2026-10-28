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

use exports::workshop::toolbox::web::{Error, FetchArgs, Guest, PostArgs, Response};
use http_client::SendError;
use wasi::http::types::ErrorCode;

/// Why a request produced no response, as this tool's typed error.
fn send_error(url: &str, e: SendError) -> Error {
    match e {
        SendError::Invalid(m) => Error::InvalidArgument(m),
        SendError::Http(ErrorCode::HttpRequestDenied) => {
            Error::AccessDenied(format!("{url}: this tool may not reach that host"))
        }
        SendError::Http(
            code @ (ErrorCode::DnsTimeout
            | ErrorCode::DnsError(_)
            | ErrorCode::DestinationNotFound
            | ErrorCode::DestinationUnavailable
            | ErrorCode::DestinationIpProhibited
            | ErrorCode::DestinationIpUnroutable
            | ErrorCode::ConnectionRefused
            | ErrorCode::ConnectionTerminated
            | ErrorCode::ConnectionTimeout
            | ErrorCode::ConnectionReadTimeout
            | ErrorCode::ConnectionWriteTimeout
            | ErrorCode::ConnectionLimitReached
            | ErrorCode::TlsProtocolError
            | ErrorCode::TlsCertificateError
            | ErrorCode::TlsAlertReceived(_)),
        ) => Error::Network(format!("{url}: {code:?}")),
        SendError::Http(code) => Error::Other(Some(format!("{url}: {code:?}"))),
    }
}

/// Send one request and return what the server answered, whatever its status.
async fn send(method: &str, url: &str, body: String) -> Result<Response, Error> {
    let (status, bytes) = http_client::send(method, url, &[], body.into_bytes())
        .await
        .map_err(|e| send_error(url, e))?;
    Ok(Response { status, text: String::from_utf8_lossy(&bytes).into_owned() })
}

struct Web;

impl Guest for Web {
    async fn fetch(args: FetchArgs) -> Result<Response, Error> {
        send("GET", &args.url, String::new()).await
    }

    async fn post(args: PostArgs) -> Result<Response, Error> {
        send("POST", &args.url, args.body).await
    }
}

export!(Web);
