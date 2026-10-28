//! reports: saves named text documents through wasmCloud's blobstore.
//!
//! The only capability this component imports is wasmcloud:blobstore (async,
//! WASI 0.3: object data travels as a stream). The container name comes from
//! REPORT_CONTAINER (default "reports"); the host decides what backs it (the
//! cluster's NATS JetStream object store on Kubernetes or `wash host`).

wit_bindgen::generate!({
    path: "../../wit",
    world: "workshop:toolbox/reports-tool",
    generate_all,
});

use exports::workshop::toolbox::reports::{Error, Guest, SaveArgs};
use wasmcloud::blobstore::blobstore;

fn env(name: &str) -> Option<String> {
    wasi::cli::environment::get_environment()
        .into_iter()
        .find_map(|(k, v)| (k == name).then_some(v))
}

/// The object store's typed error, as this tool's typed error.
fn store_error(container: &str, e: blobstore::Error) -> Error {
    match e {
        blobstore::Error::AccessDenied => Error::AccessDenied(format!("container {container}")),
        blobstore::Error::Timeout => Error::Unavailable("the object store timed out".into()),
        blobstore::Error::StoreUnavailable => {
            Error::Unavailable("the object store is unreachable".into())
        }
        blobstore::Error::QuotaExceeded => {
            Error::Unavailable("the object store is over its quota".into())
        }
        blobstore::Error::Other(m) => Error::Other(Some(m)),
        other => Error::Other(Some(format!("container {container}: {other:?}"))),
    }
}

struct Reports;

impl Guest for Reports {
    async fn save(args: SaveArgs) -> Result<(), Error> {
        let SaveArgs { name, body } = args;
        if name.trim().is_empty() {
            return Err(Error::InvalidArgument("name is empty".into()));
        }
        let cname = env("REPORT_CONTAINER").unwrap_or_else(|| "reports".into());
        let err = |e| store_error(&cname, e);
        let container = if blobstore::container_exists(cname.clone()).await.map_err(err)? {
            blobstore::get_container(cname.clone()).await.map_err(err)?
        } else {
            blobstore::create_container(cname.clone()).await.map_err(err)?
        };
        let (mut tx, rx) = wit_stream::new();
        wit_bindgen::spawn_local(async move {
            tx.write_all(body.into_bytes()).await;
        });
        container.write_data(name, rx).await.map_err(err)
    }
}

export!(Reports);
