//! Outgoing HTTP over wasi:http/client@0.3.0, for the components that send
//! requests (`#[path = "../../common/http_client.rs"] mod http_client;`).

use crate::wasi::http::types::{Fields, Method, Request, Response, Scheme};

/// Send one request through wasi:http/client and return the status and body.
/// The host checks the URL's host against this component's allowedHosts
/// before anything leaves.
#[allow(dead_code)]
pub async fn send(
    method: &str,
    url: &str,
    headers: &[(&str, &str)],
    body: Vec<u8>,
) -> Result<(u16, Vec<u8>), String> {
    let (scheme, rest) = match url.split_once("://") {
        Some(("https", rest)) => (Scheme::Https, rest),
        Some(("http", rest)) => (Scheme::Http, rest),
        _ => return Err(format!("unsupported URL {url}")),
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
    .map_err(|e| format!("headers: {e:?}"))?;
    let (mut body_tx, body_rx) = crate::wit_stream::new();
    let (trailers_tx, trailers_rx) = crate::wit_future::new(|| Ok(None));
    let (request, _sent) = Request::new(fields, Some(body_rx), trailers_rx, None);
    let method = match method {
        "GET" => Method::Get,
        "POST" => Method::Post,
        other => Method::Other(other.to_string()),
    };
    request.set_method(&method).map_err(|()| "bad method".to_string())?;
    request.set_scheme(Some(&scheme)).map_err(|()| "bad scheme".to_string())?;
    request
        .set_authority(Some(authority))
        .map_err(|()| format!("bad host in {url}"))?;
    request
        .set_path_with_query(Some(path))
        .map_err(|()| format!("bad path in {url}"))?;
    wit_bindgen::spawn_local(async move {
        body_tx.write_all(body).await;
        drop(body_tx);
        let _ = trailers_tx.write(Ok(None)).await;
    });
    let response = crate::wasi::http::client::send(request)
        .await
        .map_err(|e| format!("{e:?}"))?;
    let status = response.get_status_code();
    let (_done_tx, done_rx) = crate::wit_future::new(|| Ok(()));
    let (body, _trailers) = Response::consume_body(response, done_rx);
    Ok((status, body.collect().await))
}
