//! tools-probe (spike): answers HTTP by exercising `tools`. The body is
//! `list`, or `<tool> <argument>` to call one.

wit_bindgen::generate!({ path: "../../wit", world: "workshop:toolbox/tools-probe", generate_all });

#[path = "../../common/http.rs"]
mod http;

use exports::wasi::http::handler::Guest;
use wasi::http::types::{ErrorCode, Request, Response};
use workshop::toolbox::tools;

struct Probe;

impl Guest for Probe {
    async fn handle(req: Request) -> Result<Response, ErrorCode> {
        let body = String::from_utf8_lossy(&http::request_body(req).await).trim().to_string();
        let text = if body == "list" {
            match tools::list().await {
                Ok(ts) => ts.iter().map(|t| format!("{} ({}): {}\n", t.name, t.argument, t.description)).collect(),
                Err(e) => format!("list error: {e:?}\n"),
            }
        } else {
            let (name, arg) = body.split_once(' ').unwrap_or((body.as_str(), ""));
            match tools::call(name.to_string(), arg.to_string()).await {
                Ok(out) => format!("ok: {out}\n"),
                Err(e) => format!("error: {e:?}\n"),
            }
        };
        http::text_response(200, text)
    }
}

export!(Probe);
