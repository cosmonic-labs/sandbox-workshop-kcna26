# Familiar tools, built as components

`curl`, `sqlite3` and `git`, compiled to WebAssembly components. Each one
reaches only what its host grants it: `curl` gets the hosts in its
`allowedHosts`, `sqlite3` and `git` get the folders they are given.

Build all three from this folder (the dev container has everything they need,
including wasi-sdk for SQLite's C code):

```sh
cargo build --release
ls target/wasm32-wasip2/release/*.wasm
```

After the first build, a change to one tool rebuilds in about a second. For
the smallest files, build with `--profile small`.

| Tool | Built on | Subset |
|---|---|---|
| `curl` | [wstd](https://crates.io/crates/wstd) over `wasi:http` | `-X -H -d -i -s -S -v -o -L -f --max-redirs` |
| `sqlite3` | [rusqlite](https://crates.io/crates/rusqlite) with bundled SQLite | `sqlite3 <db> <sql>` |
| `git` | [gitoxide](https://crates.io/crates/gix), read only | `log show diff blame ls-files ls-tree grep` |
