//! talks-api: an ordinary REST service over the conference talks, standing
//! in for a service a team already runs. It knows nothing about agents or
//! MCP. The talks MCP server (components/talks-mcp) calls it over HTTP, the
//! same as any other client would.
//!
//!   GET    /talks[?track=<track>]   the talks, in schedule order
//!   GET    /talks/<id>              one talk, and whether it is a favorite
//!   PUT    /talks/<id>/favorite     mark it a favorite
//!   DELETE /talks/<id>/favorite     unmark it
//!
//! The talks are the same invented schedule as databases/talks.sql. The
//! favorites are this service's own data: a file in the folder its manifest
//! mounts at /data, which no other workload is granted.

wit_bindgen::generate!({
    path: "../../wit",
    world: "workshop:toolbox/talks-api",
    generate_all,
});

#[path = "../../common/http.rs"]
mod http;

use exports::wasi::http::handler::Guest;
use serde_json::{Value, json};
use wasi::http::types::{ErrorCode, Method, Request, Response};

/// Where this service keeps its favorites.
const FAVORITES: &str = "/data/favorites.json";

/// (title, speaker, track, room, day, start), id = index + 1.
const TALKS: &[(&str, &str, &str, &str, &str, &str)] = &[
    ("Sandboxing Agents and the Code They Write", "A. Rivera", "Security", "Hall A", "Wed", "09:00"),
    ("A Capability Model for WebAssembly", "B. Chen", "Wasm", "Room 1", "Wed", "10:30"),
    ("Least Privilege for Multi-Tenant Clusters", "C. Okafor", "Security", "Hall B", "Wed", "13:00"),
    ("Running Untrusted Code at Scale", "D. Novak", "Platform", "Room 2", "Wed", "14:30"),
    ("Components over Containers", "E. Larsson", "Wasm", "Room 1", "Thu", "09:00"),
    ("Supply Chain Attacks in the Wild", "F. Haddad", "Security", "Hall A", "Thu", "10:30"),
    ("Scaling to a Million Workloads", "G. Price", "Platform", "Hall B", "Thu", "13:00"),
    ("Debugging Wasm in Production", "H. Yamamoto", "Wasm", "Room 2", "Thu", "14:30"),
    ("Secrets Without Tears", "I. Costa", "Security", "Room 1", "Fri", "09:00"),
    ("From VMs to Sandboxes", "J. Dubois", "Platform", "Hall A", "Fri", "10:30"),
];

fn talk(id: usize) -> Value {
    let (title, speaker, track, room, day, start) = TALKS[id - 1];
    json!({ "id": id, "title": title, "speaker": speaker, "track": track, "room": room, "day": day, "start": start })
}

fn favorites() -> Vec<usize> {
    std::fs::read(FAVORITES)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn set_favorite(id: usize, on: bool) -> Result<(), String> {
    let mut ids = favorites();
    ids.retain(|&i| i != id);
    if on {
        ids.push(id);
        ids.sort_unstable();
    }
    std::fs::write(FAVORITES, serde_json::to_vec(&ids).unwrap_or_default())
        .map_err(|e| format!("{FAVORITES}: {e}"))
}

fn error(status: u16, message: String) -> Result<Response, ErrorCode> {
    http::json_response(status, json!({ "error": message }).to_string())
}

struct Talks;

impl Guest for Talks {
    async fn handle(req: Request) -> Result<Response, ErrorCode> {
        let full = req.get_path_with_query().unwrap_or_default();
        let (path, query) = full.split_once('?').unwrap_or((full.as_str(), ""));
        let method = req.get_method();
        let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
        let id = parts.get(1).and_then(|s| s.parse::<usize>().ok()).filter(|&i| (1..=TALKS.len()).contains(&i));

        match (method, parts.as_slice()) {
            (Method::Get, ["talks"]) => {
                let track = query
                    .split('&')
                    .find_map(|kv| kv.strip_prefix("track="))
                    .map(|t| t.replace("%20", " ").replace('+', " "));
                let list: Vec<Value> = (1..=TALKS.len())
                    .filter(|&i| track.as_deref().is_none_or(|t| TALKS[i - 1].2.eq_ignore_ascii_case(t)))
                    .map(talk)
                    .collect();
                http::json_response(200, Value::Array(list).to_string())
            }
            (Method::Get, ["talks", _]) => match id {
                Some(id) => {
                    let mut t = talk(id);
                    t["favorite"] = json!(favorites().contains(&id));
                    http::json_response(200, t.to_string())
                }
                None => error(404, format!("no talk at {path}")),
            },
            (m @ (Method::Put | Method::Delete), ["talks", _, "favorite"]) => match id {
                Some(id) => {
                    let on = matches!(m, Method::Put);
                    if let Err(e) = set_favorite(id, on) {
                        return error(500, e);
                    }
                    let mut t = talk(id);
                    t["favorite"] = json!(on);
                    http::json_response(200, t.to_string())
                }
                None => error(404, format!("no talk at {path}")),
            },
            _ => error(404, format!("nothing at {path}")),
        }
    }
}

export!(Talks);
