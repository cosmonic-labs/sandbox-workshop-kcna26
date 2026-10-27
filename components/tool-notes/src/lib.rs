//! notes: reads (and appends to) text files under /notes.
//!
//! The code is deliberately naive: it joins whatever path it is given onto
//! /notes with no checks, and happily tries absolute paths and writes. The
//! boundary is not this code. It is the manifest: the host mounts one
//! directory, read only, for this component alone, and nothing else exists
//! as far as this component can tell. In WASI 0.3 a component sees only the
//! directories the host preopened for it (wasi:filesystem/preopens).

wit_bindgen::generate!({
    path: "../../wit",
    world: "workshop:toolbox/notes-tool",
    generate_all,
});

use exports::workshop::toolbox::notes::Guest;
use wasi::filesystem::preopens;
use wasi::filesystem::types::{Descriptor, DescriptorFlags, ErrorCode, OpenFlags, PathFlags};

const ROOT: &str = "/notes";

/// Naive on purpose: absolute paths are used as is, relative ones are joined
/// onto /notes, ".." and all.
fn full_path(path: &str) -> String {
    if path.starts_with('/') { path.to_string() } else { format!("{ROOT}/{path}") }
}

/// The preopened directory a path lies under, and the rest of the path. A
/// path under no preopen does not exist, as far as this component can tell.
fn resolve(full: &str) -> Result<(Descriptor, String), ErrorCode> {
    for (dir, mount) in preopens::get_directories() {
        let mount = mount.trim_end_matches('/');
        if let Some(rest) = full.strip_prefix(mount).and_then(|r| r.strip_prefix('/')) {
            return Ok((dir, rest.to_string()));
        }
    }
    Err(ErrorCode::NoEntry)
}

fn describe(e: ErrorCode) -> &'static str {
    match e {
        ErrorCode::NoEntry => "No such file or directory",
        ErrorCode::NotPermitted => "Operation not permitted",
        ErrorCode::Access => "Permission denied",
        ErrorCode::ReadOnly => "Read-only file system",
        _ => "error",
    }
}

async fn open(full: &str, flags: DescriptorFlags) -> Result<Descriptor, ErrorCode> {
    let (dir, rest) = resolve(full)?;
    dir.open_at(PathFlags::empty(), rest, OpenFlags::empty(), flags).await
}

struct Notes;

impl Guest for Notes {
    async fn read(path: String) -> Result<String, String> {
        let full = full_path(&path);
        let fail = |e| format!("read {full}: {}", describe(e));
        let file = open(&full, DescriptorFlags::READ).await.map_err(fail)?;
        let (data, done) = file.read_via_stream(0);
        let bytes = data.collect().await;
        done.await.map_err(fail)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    async fn append(path: String, line: String) -> Result<(), String> {
        let full = full_path(&path);
        let fail = |e| format!("append to {full}: {}", describe(e));
        let file = open(&full, DescriptorFlags::WRITE).await.map_err(fail)?;
        let (mut tx, rx) = wit_stream::new();
        let done = file.append_via_stream(rx);
        tx.write_all(format!("{line}\n").into_bytes()).await;
        drop(tx);
        done.await.map_err(fail)
    }
}

export!(Notes);
