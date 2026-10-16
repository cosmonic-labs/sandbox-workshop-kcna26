//! remember: a memory tool backed by wasi:keyvalue.
//!
//! The only capability this component imports is wasi:keyvalue/store. The
//! bucket name comes from the MEMORY_BUCKET environment variable (default
//! "agent-memory"); the host decides what backs it (NATS JetStream KV under
//! `wash host`, memory or a directory under `wash dev`).

wit_bindgen::generate!({
    path: "../../wit",
    world: "workshop:toolbox/tool-remember",
    generate_all,
});

use exports::workshop::toolbox::tool::Guest;
use serde_json::Value;
use wasi::keyvalue::store;
use workshop::toolbox::types::ToolDef;

struct Remember;

/// NATS KV keys allow only [A-Za-z0-9_=/.-], so "my favorite color" is stored
/// as "my_favorite_color". (In-memory and filesystem backends accept spaces;
/// portable keys avoid surprises when the backend changes.)
fn to_key(k: &str) -> String {
    k.trim()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || "-_=/".contains(c) { c } else { '_' })
        .collect()
}

fn from_key(k: &str) -> String {
    k.replace('_', " ")
}

fn bucket() -> Result<store::Bucket, String> {
    let name = std::env::var("MEMORY_BUCKET").unwrap_or_else(|_| "agent-memory".into());
    store::open(&name).map_err(|e| format!("open bucket {name}: {e:?}"))
}

impl Guest for Remember {
    fn describe() -> ToolDef {
        ToolDef {
            name: "remember".into(),
            description: "Store, fetch or list facts that persist across conversations.".into(),
            parameters: r#"{"type":"object","properties":{"op":{"enum":["set","get","list"]},"key":{"type":"string"},"value":{"type":"string"}},"required":["op"]}"#.into(),
        }
    }

    fn call(arguments: String) -> Result<String, String> {
        let args: Value = serde_json::from_str(&arguments).map_err(|e| e.to_string())?;
        let s = |k: &str| args.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
        let b = bucket()?;
        match s("op").as_str() {
            "set" => {
                b.set(&to_key(&s("key")), s("value").as_bytes()).map_err(|e| format!("{e:?}"))?;
                Ok(format!("remembered {} = {}", s("key"), s("value")))
            }
            "get" => match b.get(&to_key(&s("key"))).map_err(|e| format!("{e:?}"))? {
                Some(v) => Ok(String::from_utf8_lossy(&v).into_owned()),
                None => Ok(format!("nothing stored for {}", s("key"))),
            },
            "list" => {
                let keys = b.list_keys(None).map_err(|e| format!("{e:?}"))?.keys;
                if keys.is_empty() {
                    return Ok("I do not remember anything yet".into());
                }
                let mut facts = Vec::new();
                for k in keys {
                    if let Some(v) = b.get(&k).map_err(|e| format!("{e:?}"))? {
                        facts.push(format!("{} = {}", from_key(&k), String::from_utf8_lossy(&v)));
                    }
                }
                Ok(facts.join("; "))
            }
            other => Err(format!("unknown op {other:?}")),
        }
    }
}

export!(Remember);
