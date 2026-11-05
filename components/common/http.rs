//! Small helpers over wasi:http@0.3.0, shared by the components that serve
//! HTTP (`#[path = "../../common/http.rs"] mod http;`). In WASI 0.3 a
//! body is a `stream<u8>` and the trailers are a `future`, so even a plain
//! text response is a stream written from a spawned task.

use crate::wasi::http::types::{ErrorCode, Fields, Request, Response};

/// The whole body of an incoming request.
#[allow(dead_code)]
pub async fn request_body(req: Request) -> Vec<u8> {
    let (_done_tx, done_rx) = crate::wit_future::new(|| Ok(()));
    let (body, _trailers) = Request::consume_body(req, done_rx);
    body.collect().await
}

/// A response with a text body.
#[allow(dead_code)]
pub fn text_response(status: u16, text: String) -> Result<Response, ErrorCode> {
    typed_response(status, "text/plain; charset=utf-8", false, text)
}

/// A response with an HTML page, for the pages a browser opens.
#[allow(dead_code)]
pub fn html_response(status: u16, page: &str) -> Result<Response, ErrorCode> {
    typed_response(status, "text/html; charset=utf-8", true, page.to_string())
}

/// A response with a JSON body.
#[allow(dead_code)]
pub fn json_response(status: u16, json: String) -> Result<Response, ErrorCode> {
    typed_response(status, "application/json", true, json)
}

/// A redirect to `location`, so a tool that follows redirects makes a second
/// request the host checks the same way.
#[allow(dead_code)]
pub fn redirect_response(status: u16, location: &str) -> Result<Response, ErrorCode> {
    redirect_text(status, location, format!("redirecting to {location}\n"))
}

/// A redirect to `location` whose body says something of its own, for a
/// reader using curl.
#[allow(dead_code)]
pub fn redirect_text(status: u16, location: &str, text: String) -> Result<Response, ErrorCode> {
    let list = vec![
        ("content-type".to_string(), b"text/plain; charset=utf-8".to_vec()),
        ("location".to_string(), location.as_bytes().to_vec()),
        ("cache-control".to_string(), b"no-store".to_vec()),
    ];
    let headers = Fields::from_list(&list)
        .map_err(|e| ErrorCode::InternalError(Some(format!("headers: {e:?}"))))?;
    let (mut body_tx, body_rx) = crate::wit_stream::new();
    let (trailers_tx, trailers_rx) = crate::wit_future::new(|| Ok(None));
    wit_bindgen::spawn_local(async move {
        body_tx.write_all(text.into_bytes()).await;
        drop(body_tx);
        let _ = trailers_tx.write(Ok(None)).await;
    });
    let (response, _sent) = Response::new(headers, Some(body_rx), trailers_rx);
    response
        .set_status_code(status)
        .map_err(|()| ErrorCode::InternalError(Some(format!("bad status {status}"))))?;
    Ok(response)
}

fn typed_response(
    status: u16,
    content_type: &str,
    no_store: bool,
    text: String,
) -> Result<Response, ErrorCode> {
    let mut list = vec![("content-type".to_string(), content_type.as_bytes().to_vec())];
    if no_store {
        list.push(("cache-control".to_string(), b"no-store".to_vec()));
    }
    let headers = Fields::from_list(&list)
        .map_err(|e| ErrorCode::InternalError(Some(format!("headers: {e:?}"))))?;
    let (mut body_tx, body_rx) = crate::wit_stream::new();
    let (trailers_tx, trailers_rx) = crate::wit_future::new(|| Ok(None));
    wit_bindgen::spawn_local(async move {
        body_tx.write_all(text.into_bytes()).await;
        drop(body_tx);
        let _ = trailers_tx.write(Ok(None)).await;
    });
    let (response, _sent) = Response::new(headers, Some(body_rx), trailers_rx);
    response
        .set_status_code(status)
        .map_err(|()| ErrorCode::InternalError(Some(format!("bad status {status}"))))?;
    Ok(response)
}
