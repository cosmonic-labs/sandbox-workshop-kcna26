# Step 2: Build curl, grant it one host

**Folder:** `steps/02-build-curl`

`curl` is a component like any other. What it can reach isn't in its code: it
is the `allowedHosts` line in the manifest, which this step sets to one host.

## Build it yourself (optional)

```sh
make build
make push-build
```

`curl` is pure Rust, so it needs only Rust 1.98.1 with the `wasm32-wasip2`
target (`make doctor` checks). The prebuilt tool in `artifacts/` works the same.
To make it yours, add a default `user-agent` or a short flag in
`components/tool-curl/src/cli.rs`, then build and push again.

## Look inside: no sockets

```sh
wasm-tools component wit artifacts/tool_curl.wasm \
  | grep -E 'wasi:(http/client|sockets)'
```

```
  import wasi:http/client@0.3.0;
```

`curl` imports an HTTP client and no sockets, so every request goes through the
host, which checks it against the grant. (Its full import list also has the
command line basics Rust's standard library needs, and a filesystem for `-o`
that the manifest limits to one folder.)

## Deploy

```sh
kubectl apply -f steps/02-build-curl/workload.yaml \
  && sleep 2 && kubectl wait --for=condition=Ready \
  workloaddeployment/toolbox-agent
```

The grant:

```yaml
- name: curl
  localResources:
    allowedHosts: [api.workshop-sites.svc]
```

## Ask curl

Type each prompt into the chat at http://agent.localhost:18180, or run the curl command under it.

**1. Fetch the granted host:** `fetch http://api.workshop-sites.svc/data`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'fetch http://api.workshop-sites.svc/data'
```

The host is in curl's `allowedHosts`, so the toy site answers. Step 3 tries everywhere else.

Full transcript: [`expected-output.txt`](expected-output.txt).
