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

use exports::workshop::toolbox::reports::Guest;
use wasmcloud::blobstore::blobstore;

fn env(name: &str) -> Option<String> {
    wasi::cli::environment::get_environment()
        .into_iter()
        .find_map(|(k, v)| (k == name).then_some(v))
}

struct Reports;

impl Guest for Reports {
    async fn save(name: String, body: String) -> Result<(), String> {
        let cname = env("REPORT_CONTAINER").unwrap_or_else(|| "reports".into());
        let err = |e| format!("{e:?}");
        let container = if blobstore::container_exists(cname.clone()).await.map_err(err)? {
            blobstore::get_container(cname).await.map_err(err)?
        } else {
            blobstore::create_container(cname).await.map_err(err)?
        };
        let (mut tx, rx) = wit_stream::new();
        wit_bindgen::spawn_local(async move {
            tx.write_all(body.into_bytes()).await;
        });
        container.write_data(name, rx).await.map_err(err)
    }
}

export!(Reports);
