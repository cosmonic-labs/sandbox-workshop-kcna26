//! memory: facts stored under keys, backed by wasi:keyvalue.
//!
//! The only capability this component imports is wasi:keyvalue/store. The
//! bucket name comes from the MEMORY_BUCKET environment variable (default
//! "agent-memory"); the host decides what backs it (NATS JetStream KV under
//! `wash host`, memory or a directory under `wash dev`).

wit_bindgen::generate!({
    path: "../../wit",
    world: "workshop:toolbox/memory-tool",
    generate_all,
});

use exports::workshop::toolbox::memory::{Entry, Guest};
use wasi::keyvalue::store;

struct Memory;

/// NATS KV keys allow only [A-Za-z0-9_=/.-], so "my favorite color" is stored
/// as "my_favorite_color". (In memory and filesystem backends accept spaces;
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

fn text(bytes: Vec<u8>) -> String {
    String::from_utf8_lossy(&bytes).into_owned()
}

impl Guest for Memory {
    fn remember(key: String, fact: String) -> Result<(), String> {
        bucket()?.set(&to_key(&key), fact.as_bytes()).map_err(|e| format!("{e:?}"))
    }

    fn recall(key: String) -> Result<Option<String>, String> {
        let value = bucket()?.get(&to_key(&key)).map_err(|e| format!("{e:?}"))?;
        Ok(value.map(text))
    }

    fn recall_all() -> Result<Vec<Entry>, String> {
        let b = bucket()?;
        let keys = b.list_keys(None).map_err(|e| format!("{e:?}"))?.keys;
        let mut entries = Vec::new();
        for k in keys {
            if let Some(v) = b.get(&k).map_err(|e| format!("{e:?}"))? {
                entries.push(Entry { key: from_key(&k), fact: text(v) });
            }
        }
        Ok(entries)
    }
}

export!(Memory);
