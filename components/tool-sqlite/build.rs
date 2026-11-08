//! SQLite's C code expects mmap, getpid, signals and process clocks. wasi-sdk
//! ships emulations of them as separate libraries; link them for wasm only.
//! The C compiler itself comes from CC_wasm32_wasip2 (see make build-all).
//!
//! Only those libraries are copied onto the link path, not wasi-sdk's whole
//! sysroot: its libc.a would take the place of Rust's and bring in the
//! wasi:cli/command world, sockets and a run export included.
fn main() {
    println!("cargo:rerun-if-env-changed=WASI_SDK_PATH");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("wasi") {
        return;
    }
    let sdk = std::env::var("WASI_SDK_PATH").unwrap_or_else(|_| "/opt/wasi-sdk".into());
    let lib_dir = std::path::Path::new(&sdk).join("share/wasi-sysroot/lib/wasm32-wasip2");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    for lib in ["mman", "getpid", "signal", "process-clocks"] {
        let name = format!("libwasi-emulated-{lib}.a");
        let src = lib_dir.join(&name);
        println!("cargo:rerun-if-changed={}", src.display());
        std::fs::copy(&src, out.join(&name))
            .unwrap_or_else(|e| panic!("copy {}: {e}", src.display()));
        println!("cargo:rustc-link-lib=static=wasi-emulated-{lib}");
    }
    println!("cargo:rustc-link-search=native={}", out.display());
}
