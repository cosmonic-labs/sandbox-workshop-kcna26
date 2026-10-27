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
    let headers = Fields::from_list(&[(
        "content-type".to_string(),
        b"text/plain; charset=utf-8".to_vec(),
    )])
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
