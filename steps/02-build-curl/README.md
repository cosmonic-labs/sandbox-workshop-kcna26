# Step 2: build and personalize curl

**Folder:** `steps/02-build-curl`

The `curl` tool is a component like any other. It exports one function and
imports only an HTTP client:

```sh
wasm-tools component wit artifacts/tool_curl.wasm | grep -E 'import|export'
```

It exports `workshop:toolbox/curl` and imports `wasi:http/client`, plus the
command line basics Rust's standard library needs. It imports no filesystem
write it does not use, no sockets and no environment. What `curl` can reach is
not in the code: it is the `allowedHosts` list in the manifest, which this step
sets to one host.

**Build it yourself** (optional; needs Rust 1.98.1 with the `wasm32-wasip2`
target, which `make doctor` checks). `curl` is pure Rust, so no C toolchain is
needed:

```sh
make build
make push-build
```

Open `components/tool-curl/src/cli.rs` and give it a personal touch, such as a
default `user-agent` header or a new short flag, then build and push again.

**Run:**

```sh
kubectl apply -f steps/02-build-curl/workload.yaml && sleep 2 && kubectl wait --for=condition=Ready workloaddeployment/toolbox-agent
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d 'fetch http://api.workshop-sites.svc/data'
```

**Expected:** the agent reaches `api.workshop-sites.svc`, the one host its
manifest grants. The next step tries everywhere else.

Full transcript: `expected-output.txt`.
