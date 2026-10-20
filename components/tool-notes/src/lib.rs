//! notes: reads (and appends to) text files under /notes.
//!
//! The code is deliberately naive: it joins whatever path it is given onto
//! /notes with no checks, and happily tries absolute paths and writes. The
//! boundary is not this code. It is the manifest: the host mounts one
//! directory, read only, for this component alone, and nothing else exists
//! as far as this component can tell.

wit_bindgen::generate!({
    path: "../../wit",
    world: "workshop:toolbox/notes-tool",
    generate_all,
});

use exports::workshop::toolbox::notes::Guest;
use std::io::Write;

const ROOT: &str = "/notes";

/// Naive on purpose: absolute paths are used as is, relative ones are joined
/// onto /notes, ".." and all.
fn full_path(path: &str) -> String {
    if path.starts_with('/') { path.to_string() } else { format!("{ROOT}/{path}") }
}

struct Notes;

impl Guest for Notes {
    fn read(path: String) -> Result<String, String> {
        let full = full_path(&path);
        std::fs::read_to_string(&full).map_err(|e| format!("read {full}: {e}"))
    }

    fn append(path: String, line: String) -> Result<(), String> {
        let full = full_path(&path);
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&full)
            .map_err(|e| format!("append to {full}: {e}"))?;
        writeln!(file, "{line}").map_err(|e| format!("append to {full}: {e}"))
    }
}

export!(Notes);
