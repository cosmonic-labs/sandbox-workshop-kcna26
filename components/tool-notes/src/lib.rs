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

use exports::workshop::toolbox::notes::{AppendArgs, Error, Guest, ReadArgs};
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

/// The filesystem's error for `full`, as this tool's typed error. The message
/// says what happened to the path, in the words `ls` or `cat` would use.
fn fs_error(full: &str, e: ErrorCode) -> Error {
    match e {
        ErrorCode::NoEntry => Error::NotFound(format!("{full}: No such file or directory")),
        ErrorCode::NotPermitted => Error::AccessDenied(format!("{full}: Operation not permitted")),
        ErrorCode::Access => Error::AccessDenied(format!("{full}: Permission denied")),
        ErrorCode::ReadOnly => Error::AccessDenied(format!("{full}: Read-only file system")),
        other => Error::Other(Some(format!("{full}: {other:?}"))),
    }
}

/// The full path for `path`, or invalid-argument when it is empty.
fn checked(path: &str) -> Result<String, Error> {
    if path.trim().is_empty() {
        return Err(Error::InvalidArgument("path is empty".into()));
    }
    Ok(full_path(path))
}

async fn open(full: &str, flags: DescriptorFlags) -> Result<Descriptor, ErrorCode> {
    let (dir, rest) = resolve(full)?;
    dir.open_at(PathFlags::empty(), rest, OpenFlags::empty(), flags).await
}

struct Notes;

impl Guest for Notes {
    async fn read(args: ReadArgs) -> Result<String, Error> {
        let full = checked(&args.path)?;
        let fail = |e| fs_error(&full, e);
        let file = open(&full, DescriptorFlags::READ).await.map_err(fail)?;
        let (data, done) = file.read_via_stream(0);
        let bytes = data.collect().await;
        done.await.map_err(fail)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    async fn append(args: AppendArgs) -> Result<(), Error> {
        let full = checked(&args.path)?;
        let fail = |e| fs_error(&full, e);
        let file = open(&full, DescriptorFlags::WRITE).await.map_err(fail)?;
        let (mut tx, rx) = wit_stream::new();
        let done = file.append_via_stream(rx);
        tx.write_all(format!("{}\n", args.line).into_bytes()).await;
        drop(tx);
        done.await.map_err(fail)
    }
}

export!(Notes);
