//! memory: facts stored under keys, backed by wasmCloud's keyvalue interface.
//!
//! The only capability this component imports is wasmcloud:keyvalue/store
//! (async, WASI 0.3). The bucket name comes from the MEMORY_BUCKET environment
//! variable (default "agent-memory"); the host decides what backs it (the
//! cluster's NATS JetStream key value store in this workshop).

wit_bindgen::generate!({
    path: "../../wit",
    world: "workshop:toolbox/memory-tool",
    generate_all,
});

use exports::workshop::toolbox::memory::{Entry, Error, Guest, RecallArgs, RememberArgs};
use wasmcloud::keyvalue::store;

struct Memory;

/// NATS KV keys allow only [A-Za-z0-9_=/.-], so "my favorite color" is stored
/// as "my_favorite_color". (In memory and filesystem backends accept spaces;
/// portable keys avoid surprises when the backend changes.)
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

fn env(name: &str) -> Option<String> {
    wasi::cli::environment::get_environment()
        .into_iter()
        .find_map(|(k, v)| (k == name).then_some(v))
}

fn bucket_name() -> String {
    env("MEMORY_BUCKET").unwrap_or_else(|| "agent-memory".into())
}

/// The store's typed error, as this tool's typed error.
fn store_error(e: store::Error) -> Error {
    let bucket = bucket_name();
    match e {
        store::Error::NoSuchStore => Error::NoSuchStore(format!("bucket {bucket}")),
        store::Error::AccessDenied => Error::AccessDenied(format!("bucket {bucket}")),
        store::Error::InvalidArgument => Error::InvalidArgument("the store refused the key".into()),
        store::Error::Timeout => Error::Unavailable("the store timed out".into()),
        store::Error::StoreUnavailable => Error::Unavailable("the store is unreachable".into()),
        store::Error::QuotaExceeded => Error::Unavailable("the store is over its quota".into()),
        store::Error::Other(m) => Error::Other(Some(m)),
        other => Error::Other(Some(format!("{other:?}"))),
    }
}

async fn bucket() -> Result<store::Bucket, Error> {
    store::open(bucket_name()).await.map_err(store_error)
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
