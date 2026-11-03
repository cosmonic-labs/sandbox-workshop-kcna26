use crate::FileTime;
use std::fs;
use std::io;
use std::path::Path;

pub fn set_symlink_file_times(_p: &Path, _atime: FileTime, _mtime: FileTime) -> io::Result<()> {
    Err(io::Error::new(io::ErrorKind::Other, "Wasm not implemented"))
}

// git-spike patch: WASI exposes timestamps through std::fs::Metadata, so use them
// instead of panicking. Unknown times map to the epoch.
fn st(t: io::Result<std::time::SystemTime>) -> Option<FileTime> {
    t.ok().map(FileTime::from_system_time)
}

pub fn from_last_modification_time(meta: &fs::Metadata) -> FileTime {
    st(meta.modified()).unwrap_or_else(FileTime::zero)
}

pub fn from_last_access_time(meta: &fs::Metadata) -> FileTime {
    st(meta.accessed()).unwrap_or_else(FileTime::zero)
}

pub fn from_creation_time(meta: &fs::Metadata) -> Option<FileTime> {
    st(meta.created())
}

pub fn open(path: &Path) -> io::Result<fs::File> {
    fs::File::open(path)
}
