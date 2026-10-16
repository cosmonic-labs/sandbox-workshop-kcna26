//! save_report: writes a report object through wasi:blobstore.
//!
//! The only capability this component imports is wasi:blobstore. The
//! container name comes from REPORT_CONTAINER (default "reports"); the host
//! decides what backs it (NATS JetStream object store under `wash host`, a
//! directory or memory under `wash dev`).

wit_bindgen::generate!({
    path: "../../wit",
    world: "workshop:toolbox/tool-save-report",
    generate_all,
});

use exports::workshop::toolbox::tool::Guest;
use serde_json::Value;
use wasi::blobstore::blobstore;
use wasi::blobstore::types::OutgoingValue;
use workshop::toolbox::types::ToolDef;

struct SaveReport;

impl Guest for SaveReport {
    fn describe() -> ToolDef {
        ToolDef {
            name: "save_report".into(),
            description: "Save a text report to object storage.".into(),
            parameters: r#"{"type":"object","properties":{"name":{"type":"string"},"content":{"type":"string"}},"required":["name","content"]}"#.into(),
        }
    }

    fn call(arguments: String) -> Result<String, String> {
        let args: Value = serde_json::from_str(&arguments).map_err(|e| e.to_string())?;
        let name = args.get("name").and_then(Value::as_str).unwrap_or("report.txt");
        let content = args.get("content").and_then(Value::as_str).unwrap_or_default();
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
        container.write_data(name, &value)?;
        for chunk in content.as_bytes().chunks(4096) {
            stream
                .blocking_write_and_flush(chunk)
                .map_err(|e| format!("write: {e:?}"))?;
        }
        drop(stream);
        OutgoingValue::finish(value)?;
        Ok(format!("saved {} bytes to {cname}/{name}", content.len()))
    }
}

export!(SaveReport);
