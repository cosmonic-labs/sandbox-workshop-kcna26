//! memory: facts stored under keys, backed by wasmCloud's keyvalue interface.
//!
//! The only capability this component imports is wasmcloud:keyvalue/store
//! (async, WASI 0.3). The host decides what backs it: in this workshop, the
//! cluster's Valkey data service, under a key prefix the platform picks.
//!
//! One source file, three builds. This crate imports the store with no
//! label and gets the platform's default store. Step 3's tool-memory-team-a
//! and tool-memory-team-b build this same file with a feature that labels the
//! import with the team's name, so the platform can give each team a store of
//! its own (k8s/values.yaml, `bindings:`).

#[cfg(not(any(feature = "team-a", feature = "team-b")))]
wit_bindgen::generate!({
    path: "../../wit",
    world: "workshop:toolbox/memory-tool",
    generate_all,
});
#[cfg(not(any(feature = "team-a", feature = "team-b")))]
use wasmcloud::keyvalue::store;

#[cfg(feature = "team-a")]
wit_bindgen::generate!({
    path: "../../wit",
    world: "workshop:toolbox/memory-tool-team-a",
    generate_all,
});
#[cfg(feature = "team-a")]
use team_a as store;

#[cfg(feature = "team-b")]
wit_bindgen::generate!({
    path: "../../wit",
    world: "workshop:toolbox/memory-tool-team-b",
    generate_all,
});
#[cfg(feature = "team-b")]
use team_b as store;

use exports::workshop::toolbox::memory::{Entry, Error, Guest, RecallArgs, RememberArgs};

/// The bucket the tool opens. On this workshop's Valkey backend the name
/// selects nothing: the platform's binding decides which keys a tool sees.
/// Other backends (NATS, for one) map it to a bucket of that name.
const BUCKET: &str = "memory";

struct Memory;

/// Some backends limit key characters (NATS KV allows only [A-Za-z0-9_=/.-]),
/// so "my favorite color" is stored as "my_favorite_color". Valkey accepts any
/// key; portable keys avoid surprises when the backend changes.
fn to_key(k: &str) -> Result<String, Error> {
    let k = k.trim();
    if k.is_empty() {
        return Err(Error::InvalidArgument("key is empty".into()));
    }
    Ok(k.chars()
        .map(|c| if c.is_ascii_alphanumeric() || "-_=/".contains(c) { c } else { '_' })
        .collect())
}

fn from_key(k: &str) -> String {
    k.replace('_', " ")
}

/// The store's typed error, as this tool's typed error.
fn store_error(e: store::Error) -> Error {
    match e {
        store::Error::NoSuchStore => Error::NoSuchStore(format!("bucket {BUCKET}")),
        store::Error::AccessDenied => Error::AccessDenied(format!("bucket {BUCKET}")),
        store::Error::InvalidArgument => Error::InvalidArgument("the store refused the key".into()),
        store::Error::Timeout => Error::Unavailable("the store timed out".into()),
        store::Error::StoreUnavailable => Error::Unavailable("the store is unreachable".into()),
        store::Error::QuotaExceeded => Error::Unavailable("the store is over its quota".into()),
        store::Error::Other(m) => Error::Other(Some(m)),
        other => Error::Other(Some(format!("{other:?}"))),
    }
}

async fn bucket() -> Result<store::Bucket, Error> {
    store::open(BUCKET.into()).await.map_err(store_error)
}

fn text(bytes: Vec<u8>) -> String {
    String::from_utf8_lossy(&bytes).into_owned()
}

impl Guest for Memory {
    async fn remember(args: RememberArgs) -> Result<(), Error> {
        let key = to_key(&args.key)?;
        let b = bucket().await?;
        b.set(key, args.fact.into_bytes(), None).await.map_err(store_error)
    }

    async fn recall(args: RecallArgs) -> Result<Option<String>, Error> {
        let key = to_key(&args.key)?;
        let b = bucket().await?;
        Ok(b.get(key).await.map_err(store_error)?.map(text))
    }

    async fn recall_all() -> Result<Vec<Entry>, Error> {
        let b = bucket().await?;
        let keys = b.list_keys(None, None).await.map_err(store_error)?.keys;
        let mut entries = Vec::new();
        for k in keys {
            if let Some(v) = b.get(k.clone()).await.map_err(store_error)? {
                entries.push(Entry { key: from_key(&k), fact: text(v) });
            }
        }
        Ok(entries)
    }
}

export!(Memory);
