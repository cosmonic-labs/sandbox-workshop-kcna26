//! SQLite's C code expects mmap, getpid, signals and process clocks. wasi-sdk
//! ships emulations of them as separate libraries; link them for wasm only.
//! The C compiler itself comes from CC_wasm32_wasip2 (see make build-all).
fn main() {
    println!("cargo:rerun-if-env-changed=WASI_SDK_PATH");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("wasi") {
        return;
    }
    let sdk = std::env::var("WASI_SDK_PATH").unwrap_or_else(|_| "/opt/wasi-sdk".into());
    println!("cargo:rustc-link-search=native={sdk}/share/wasi-sysroot/lib/wasm32-wasip2");
    for lib in ["mman", "getpid", "signal", "process-clocks"] {
        println!("cargo:rustc-link-lib=static=wasi-emulated-{lib}");
    }
}
