//! reports: saves named text documents through wasi:blobstore.
//!
//! The only capability this component imports is wasi:blobstore. The
//! container name comes from REPORT_CONTAINER (default "reports"); the host
//! decides what backs it (NATS JetStream object store under `wash host`, a
//! directory or memory under `wash dev`).

wit_bindgen::generate!({
    path: "../../wit",
    world: "workshop:toolbox/reports-tool",
    generate_all,
});

use exports::workshop::toolbox::reports::Guest;
use wasi::blobstore::blobstore;
use wasi::blobstore::types::OutgoingValue;

struct Reports;

impl Guest for Reports {
    fn save(name: String, body: String) -> Result<(), String> {
        let cname = std::env::var("REPORT_CONTAINER").unwrap_or_else(|_| "reports".into());
        let container = if blobstore::container_exists(&cname)? {
            blobstore::get_container(&cname)?
        } else {
            blobstore::create_container(&cname)?
        };
        let value = OutgoingValue::new_outgoing_value();
        let stream = value
            .outgoing_value_write_body()
            .map_err(|()| "no output stream".to_string())?;
        container.write_data(&name, &value)?;
        for chunk in body.as_bytes().chunks(4096) {
            stream
                .blocking_write_and_flush(chunk)
                .map_err(|e| format!("write: {e:?}"))?;
        }
        drop(stream);
        OutgoingValue::finish(value)
    }
}

export!(Reports);
