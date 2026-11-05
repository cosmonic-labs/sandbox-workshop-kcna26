# Step 2: Build curl, grant it one host

**Folder:** `steps/02-build-curl`

`curl` is a component like any other. Its code decides how it reaches the
network: through the host's HTTP client, with no sockets of its own. Which
hosts it may reach isn't in its code. It is the `allowedHosts` line in the
manifest, which this step sets to one host.

Everyone can use the **prebuilt** curl in `artifacts/`, and every command
below shows that first. If you have Rust, you can **build your own** instead;
each command then has a second form for your build. Both end with the same
grant.

## Build your own (optional)

Skip this section to use the prebuilt curl.

You need Rust 1.98.1 with the `wasm32-wasip2` target (`make doctor` checks).
`curl` is pure Rust, so no C toolchain is needed.

To make it yours, open `components/tool-curl/src/lib.rs` and change the
message `"saved {} bytes to {path}\n"`, for example to
`"saved {} bytes to {path} (built by me)\n"`. Then build and push:

```sh
make build
make push-build
```

`make build` writes `target/wasm32-wasip2/release/tool_curl.wasm`.
`make push-build` pushes it as `tool-curl:mine`, a tag of its own. The
prebuilt tool keeps its tag, `tool-curl:0.4.0`, so later steps are not
affected.

## Look inside

`wasm-tools component wit` prints a component's interface: what it imports
from the host and what it exports.

Prebuilt:

```sh
wasm-tools component wit artifacts/tool_curl.wasm \
  | grep -E 'import wasi:(http|sockets|filesystem)'
```

Your build:

```sh
wasm-tools component wit target/wasm32-wasip2/release/tool_curl.wasm \
  | grep -E 'import wasi:(http|sockets|filesystem)'
```

## Deploy

Prebuilt:

```sh
kubectl apply -f steps/02-build-curl/workload.yaml \
  && sleep 2 && kubectl wait --for=condition=Ready \
  workloaddeployment/toolbox-agent
```

Your build:

```sh
kubectl apply -f steps/02-build-curl/workload-mine.yaml \
  && sleep 2 && kubectl wait --for=condition=Ready \
  workloaddeployment/toolbox-agent
```

`workload-mine.yaml` is `workload.yaml` with one change: the `curl` image is
`tool-curl:mine`. Built again? Run `make push-build`, then
`kubectl delete workloaddeployment toolbox-agent` and apply again, so the host
fetches the new push.

## What the imports say

The prebuilt curl and your build print the same four lines:

```
  import wasi:http/types@0.3.0;
  import wasi:http/client@0.3.0;
  import wasi:filesystem/types@0.2.9;
  import wasi:filesystem/preopens@0.2.9;
```

`curl` imports an HTTP client and no sockets, so every request goes through the
host, which checks it against the grant. The filesystem is for `-o`, and the
manifest limits it to one folder. (The full import list also has the command
line basics Rust's standard library needs: stdout, environment, clocks.)

The grant, the same in both manifests:

```yaml
- name: curl
  localResources:
    allowedHosts: [api.workshop-sites.svc]
    volumeMounts: [{ name: curl-data, mountPath: /data }]
```

## Ask curl

Type each prompt into the chat at http://agent.localhost:18180, or run the
curl command under it.

**1. Fetch the granted host:** `fetch http://api.workshop-sites.svc/data`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'fetch http://api.workshop-sites.svc/data'
```

The host is in curl's `allowedHosts`, so the toy site answers.

**2. Save a page:** `save http://api.workshop-sites.svc/data to page.html`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'save http://api.workshop-sites.svc/data to page.html'
```

The prebuilt curl answers `saved 95 bytes to /data/page.html`. Your build
answers with the message you wrote, which shows your code is the one running.

Step 3 goes back to the prebuilt curl and tries everywhere else. To keep your
own build in later steps, change the `curl` image in that step's manifest to
`tool-curl:mine`.

Full transcript (prebuilt curl): [`expected-output.txt`](expected-output.txt).
