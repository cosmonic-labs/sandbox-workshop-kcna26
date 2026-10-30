//! Outgoing HTTP over wasi:http/client@0.3.0, for the components that send
//! requests (`#[path = "../../common/http_client.rs"] mod http_client;`).

use crate::wasi::http::types::{ErrorCode, Fields, Method, Request, Response, Scheme};

/// Why a request produced no response.
#[allow(dead_code)]
#[derive(Debug)]
pub enum SendError {
    /// The URL or a header could not be used, so nothing was sent.
    Invalid(String),
    /// wasi:http refused or failed the request. `HttpRequestDenied` means the
    /// host is not in this component's allowedHosts.
    Http(ErrorCode),
}

impl std::fmt::Display for SendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SendError::Invalid(m) => f.write_str(m),
            SendError::Http(e) => write!(f, "{e:?}"),
        }
    }
}

/// Send one request through wasi:http/client and return the status and body.
/// The host checks the URL's host against this component's allowedHosts
/// before anything leaves.
#[allow(dead_code)]
pub async fn send(
    method: &str,
    url: &str,
    headers: &[(&str, &str)],
    body: Vec<u8>,
) -> Result<(u16, Vec<u8>), SendError> {
    let invalid = SendError::Invalid;
    let (scheme, rest) = match url.split_once("://") {
        Some(("https", rest)) => (Scheme::Https, rest),
        Some(("http", rest)) => (Scheme::Http, rest),
        _ => return Err(invalid(format!("unsupported URL {url}"))),
    };
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let fields = Fields::from_list(
        &headers
            .iter()
            .map(|(k, v)| (k.to_string(), v.as_bytes().to_vec()))
            .collect::<Vec<_>>(),
    )
    .map_err(|e| invalid(format!("headers: {e:?}")))?;
    let (mut body_tx, body_rx) = crate::wit_stream::new();
    let (trailers_tx, trailers_rx) = crate::wit_future::new(|| Ok(None));
    let (request, _sent) = Request::new(fields, Some(body_rx), trailers_rx, None);
    let method = match method {
        "GET" => Method::Get,
        "POST" => Method::Post,
        other => Method::Other(other.to_string()),
    };
    request.set_method(&method).map_err(|()| invalid("bad method".to_string()))?;
    request.set_scheme(Some(&scheme)).map_err(|()| invalid("bad scheme".to_string()))?;
    request
        .set_authority(Some(authority))
        .map_err(|()| invalid(format!("bad host in {url}")))?;
    request
        .set_path_with_query(Some(path))
        .map_err(|()| invalid(format!("bad path in {url}")))?;
    wit_bindgen::spawn_local(async move {
        body_tx.write_all(body).await;
        drop(body_tx);
        let _ = trailers_tx.write(Ok(None)).await;
    });
    let response = crate::wasi::http::client::send(request).await.map_err(SendError::Http)?;
    let status = response.get_status_code();
    let (_done_tx, done_rx) = crate::wit_future::new(|| Ok(()));
    let (body, _trailers) = Response::consume_body(response, done_rx);
    Ok((status, body.collect().await))
}
