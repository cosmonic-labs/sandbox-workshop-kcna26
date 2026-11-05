//! The workshop guide: README.md and each steps/*/README.md as web pages, at
//! http://guide.localhost:18180 on the attendee's cluster.
//!
//!   GET /                  the overview (README.md) and the step list
//!   GET /steps/<folder>/   that folder's README.md
//!   GET /guide.css         the one stylesheet every page uses
//!   GET /sources           the cksum of each README the pages were built from,
//!                          so make verify can tell a stale guide
//!
//! build.rs renders the Markdown when the component is built, so the component
//! holds finished pages. It imports no files, no sockets and no HTTP client. It runs on
//! the fixtures host group (k8s/guide.yaml), beside the toy sites.

wit_bindgen::generate!({
    path: "../../wit",
    world: "workshop:toolbox/guide",
    generate_all,
});

#[path = "../../common/http.rs"]
mod http;

include!(concat!(env!("OUT_DIR"), "/pages.rs"));

use exports::wasi::http::handler::Guest;
use wasi::http::types::{ErrorCode, Request, Response};

struct Guide;

impl Guest for Guide {
    async fn handle(req: Request) -> Result<Response, ErrorCode> {
        let full = req.get_path_with_query().unwrap_or_default();
        let path = full.split(['?', '#']).next().unwrap_or("/");
        let path = if path.is_empty() { "/" } else { path };
        if let Some((_, page)) = PAGES.iter().find(|(route, _)| *route == path) {
            return http::html_response(200, page);
        }
        if path == "/guide.css" {
            return http::css_response(CSS);
        }
        if path == "/sources" {
            return http::text_response(200, SOURCES.to_string());
        }
        // /steps/02-curl, /steps/02-curl/README.md and /README.md: the page.
        let dir = path.strip_suffix("README.md").unwrap_or(path).trim_end_matches('/');
        let want = format!("{dir}/");
        if let Some((route, _)) = PAGES.iter().find(|(route, _)| *route == want) {
            return http::redirect_response(301, route);
        }
        http::html_response(404, NOT_FOUND)
    }
}

export!(Guide);
