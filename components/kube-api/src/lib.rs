//! kube-api: a host component plugin that sends requests to the Kubernetes
//! API server with a service account token.
//!
//! The platform installs it on the host group (k8s/values.yaml), not a
//! workload. The token comes from the plugin's own config: the host group's
//! `secretFrom` names a service-account-token Secret, and the plugin reads its
//! `token` key through the labelled `token` import. The token never crosses to
//! the workloads that call `send`, and the plugin's own allowedHosts let it
//! reach only the API server.

mod bindings {
    #![allow(unsafe_code)]
    wit_bindgen::generate!({ path: "../../wit", world: "workshop:toolbox/kube-api-plugin", generate_all });
}

use bindings::exports::workshop::toolbox::kube_api::{Guest, Response};
use bindings::wasi::http::types::{Fields, Method, Request, Response as HttpResponse, Scheme};
use bindings::wasmcloud::secrets::reveal::reveal;
use bindings::wasmcloud::secrets::store::SecretValue;

/// The API server, as seen from a pod. allowedHosts names only this.
const API_SERVER: &str = "kubernetes.default.svc";

struct Component;

impl Guest for Component {
    async fn send(method: String, path_and_query: String, body: Vec<u8>) -> Result<Response, String> {
        if !path_and_query.starts_with('/') {
            return Err(format!("path must start with '/': {path_and_query}"));
        }
        let token = match reveal(&bindings::token::get().await).await {
            SecretValue::String(s) => s,
            SecretValue::Bytes(b) => String::from_utf8(b).map_err(|_| "token is not UTF-8".to_string())?,
        };
        let auth = format!("Bearer {}", token.trim());
        let fields = Fields::from_list(&[
            ("authorization".to_string(), auth.into_bytes()),
            ("accept".to_string(), b"application/json".to_vec()),
            ("content-type".to_string(), b"application/json".to_vec()),
        ])
        .map_err(|e| format!("headers: {e:?}"))?;
        let (mut body_tx, body_rx) = bindings::wit_stream::new();
        let (trailers_tx, trailers_rx) = bindings::wit_future::new(|| Ok(None));
        let (request, _sent) = Request::new(fields, Some(body_rx), trailers_rx, None);
        let method = match method.as_str() {
            "GET" => Method::Get,
            "POST" => Method::Post,
            "PUT" => Method::Put,
            "PATCH" => Method::Patch,
            "DELETE" => Method::Delete,
            other => Method::Other(other.to_string()),
        };
        request.set_method(&method).map_err(|()| "bad method".to_string())?;
        request.set_scheme(Some(&Scheme::Https)).map_err(|()| "bad scheme".to_string())?;
        request.set_authority(Some(API_SERVER)).map_err(|()| "bad authority".to_string())?;
        request
            .set_path_with_query(Some(&path_and_query))
            .map_err(|()| format!("bad path: {path_and_query}"))?;
        wit_bindgen::spawn_local(async move {
            body_tx.write_all(body).await;
            drop(body_tx);
            let _ = trailers_tx.write(Ok(None)).await;
        });
        let response = bindings::wasi::http::client::send(request)
            .await
            .map_err(|e| format!("{e:?}"))?;
        let status = response.get_status_code();
        let (_done_tx, done_rx) = bindings::wit_future::new(|| Ok(()));
        let (body, _trailers) = HttpResponse::consume_body(response, done_rx);
        Ok(Response { status, body: body.collect().await })
    }
}

mod export {
    #![allow(unsafe_code)]
    use super::{Component, bindings};
    bindings::export!(Component with_types_in bindings);
}
