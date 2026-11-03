// git-spike patch: on targets without mmap (wasm32-wasip2), emulate read-only and
// copy-on-write maps by reading the file range into a heap buffer. Shared writable
// and executable maps stay unsupported.
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};

pub struct MmapInner {
    buf: Box<[u8]>,
}

impl MmapInner {
    fn read(len: usize, file: &File, offset: u64) -> io::Result<MmapInner> {
        // A real mmap leaves the file cursor untouched (gix-index hashes the same handle
        // right after mapping it), so restore the position after the copy.
        let mut f = file;
        let pos = f.stream_position()?;
        f.seek(SeekFrom::Start(offset))?;
        let mut buf = vec![0u8; len];
        let res = f.read_exact(&mut buf);
        f.seek(SeekFrom::Start(pos))?;
        res?;
        Ok(MmapInner { buf: buf.into_boxed_slice() })
    }

    pub fn map(len: usize, file: &File, offset: u64, _: bool, _: bool, _: bool) -> io::Result<MmapInner> {
        Self::read(len, file, offset)
    }

    pub fn map_exec(_: usize, _: &File, _: u64, _: bool, _: bool, _: bool) -> io::Result<MmapInner> {
        Err(io::ErrorKind::Unsupported.into())
    }

    pub fn map_mut(_: usize, _: &File, _: u64, _: bool, _: bool, _: bool) -> io::Result<MmapInner> {
        Err(io::ErrorKind::Unsupported.into())
    }

    pub fn map_copy(len: usize, file: &File, offset: u64, _: bool, _: bool, _: bool) -> io::Result<MmapInner> {
        Self::read(len, file, offset)
    }

    pub fn map_copy_read_only(
        len: usize,
        file: &File,
        offset: u64,
        _: bool,
        _: bool,
        _: bool,
    ) -> io::Result<MmapInner> {
        Self::read(len, file, offset)
    }

    pub fn map_anon(len: usize, _: bool, _: bool, _: Option<u8>, _: bool) -> io::Result<MmapInner> {
        Ok(MmapInner { buf: vec![0u8; len].into_boxed_slice() })
    }

    pub fn flush(&self, _: usize, _: usize) -> io::Result<()> {
        Ok(())
    }

    pub fn flush_async(&self, _: usize, _: usize) -> io::Result<()> {
        Ok(())
    }

    pub fn make_read_only(&mut self) -> io::Result<()> {
        Ok(())
    }

    pub fn make_exec(&mut self) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }

    pub fn make_mut(&mut self) -> io::Result<()> {
        Ok(())
    }

    #[inline]
    pub fn ptr(&self) -> *const u8 {
        self.buf.as_ptr()
    }

    #[inline]
    pub fn mut_ptr(&mut self) -> *mut u8 {
        self.buf.as_mut_ptr()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    #[allow(clippy::unnecessary_wraps)]
    pub fn check_safe_to_map(
        _file: &File,
        _offset: u64,
        _len: usize,
    ) -> Result<(), crate::MapIfSafeError> {
        Err(crate::MapIfSafeError::new_not_supported())
    }
}

pub fn file_len(file: &File) -> io::Result<u64> {
    Ok(file.metadata()?.len())
}
