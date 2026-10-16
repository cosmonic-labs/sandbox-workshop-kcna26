//! read_notes: reads (and naively appends to) files under /notes.
//!
//! The code is deliberately naive: it joins whatever path it is given onto
//! /notes with no checks, and happily tries absolute paths and writes. The
//! boundary is not this code. It is the manifest: the host preopens one
//! directory, read only, for this component alone, and nothing else exists
//! as far as this component can tell.

wit_bindgen::generate!({
    path: "../../wit",
    world: "workshop:toolbox/tool-provider",
    generate_all,
});

use exports::workshop::toolbox::tool::Guest;
use serde_json::Value;
use std::io::Write;
use workshop::toolbox::types::ToolDef;

const ROOT: &str = "/notes";

struct ReadNotes;

impl Guest for ReadNotes {
    fn describe() -> ToolDef {
        ToolDef {
            name: "read_notes".into(),
            description: "Read a file from the user's notes folder, or append a line to one.".into(),
            parameters: r#"{"type":"object","properties":{"path":{"type":"string"},"append":{"type":"string"}},"required":["path"]}"#.into(),
        }
    }

    fn call(arguments: String) -> Result<String, String> {
        let args: Value = serde_json::from_str(&arguments).map_err(|e| e.to_string())?;
        let path = args.get("path").and_then(Value::as_str).unwrap_or("todo.txt");
        // Naive on purpose: absolute paths are used as is, relative ones are
        // joined onto /notes, ".." and all.
        let full = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("{ROOT}/{path}")
        };
        if let Some(line) = args.get("append").and_then(Value::as_str) {
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&full)
                .map_err(|e| format!("append to {full}: {e}"))?;
            writeln!(f, "{line}").map_err(|e| format!("append to {full}: {e}"))?;
            return Ok(format!("appended to {full}"));
        }
        std::fs::read_to_string(&full).map_err(|e| format!("read {full}: {e}"))
    }
}

export!(ReadNotes);
