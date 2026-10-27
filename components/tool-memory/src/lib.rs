//! memory: facts stored under keys, backed by wasmCloud's keyvalue interface.
//!
//! The only capability this component imports is wasmcloud:keyvalue/store
//! (async, WASI 0.3). The bucket name comes from the MEMORY_BUCKET environment
//! variable (default "agent-memory"); the host decides what backs it (the
//! cluster's NATS JetStream key value store on Kubernetes or `wash host`).

wit_bindgen::generate!({
    path: "../../wit",
    world: "workshop:toolbox/memory-tool",
    generate_all,
});

use exports::workshop::toolbox::memory::{Entry, Guest};
use wasmcloud::keyvalue::store;

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

fn env(name: &str) -> Option<String> {
    wasi::cli::environment::get_environment()
        .into_iter()
        .find_map(|(k, v)| (k == name).then_some(v))
}

async fn bucket() -> Result<store::Bucket, String> {
    let name = env("MEMORY_BUCKET").unwrap_or_else(|| "agent-memory".into());
    store::open(name.clone()).await.map_err(|e| format!("open bucket {name}: {e:?}"))
}

fn text(bytes: Vec<u8>) -> String {
    String::from_utf8_lossy(&bytes).into_owned()
}

impl Guest for Memory {
    async fn remember(key: String, fact: String) -> Result<(), String> {
        let b = bucket().await?;
        b.set(to_key(&key), fact.into_bytes(), None).await.map_err(|e| format!("{e:?}"))
    }

    async fn recall(key: String) -> Result<Option<String>, String> {
        let b = bucket().await?;
        let value = b.get(to_key(&key)).await.map_err(|e| format!("{e:?}"))?;
        Ok(value.map(text))
    }

    async fn recall_all() -> Result<Vec<Entry>, String> {
        let b = bucket().await?;
        let keys = b.list_keys(None, None).await.map_err(|e| format!("{e:?}"))?.keys;
        let mut entries = Vec::new();
        for k in keys {
            if let Some(v) = b.get(k.clone()).await.map_err(|e| format!("{e:?}"))? {
                entries.push(Entry { key: from_key(&k), fact: text(v) });
            }
        }
        Ok(entries)
    }
}

export!(Memory);
