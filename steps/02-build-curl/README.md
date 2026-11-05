# Step 2: Build curl, grant it one host

**Folder:** `steps/02-build-curl`

`curl` is a component like any other. What it can reach isn't in its code. It
is the `allowedHosts` line in the manifest, which this step sets to one host.

You can build `curl` yourself (path A) or use the prebuilt one (path B). Both
end with the same grant.

## Path A: build it yourself

You need Rust 1.98.1 with the `wasm32-wasip2` target (`make doctor` checks).
`curl` is pure Rust, so no C toolchain is needed.

To make it yours, open `components/tool-curl/src/lib.rs` and change the
message `"saved {} bytes to {path}\n"`, for example to
`"saved {} bytes to {path} (built by me)\n"`. Then build and push:

```sh
make build
make push-build
```

`make push-build` pushes your build as `tool-curl:mine`, a tag of its own.
The prebuilt tool keeps its tag, `tool-curl:0.4.0`, so later steps are not
affected.

Look inside your build. `wasm-tools component wit` prints a component's
interface: what it imports from the host and what it exports.

```sh
wasm-tools component wit target/wasm32-wasip2/release/tool_curl.wasm \
  | grep -E 'import wasi:(http|sockets|filesystem)'
```

Deploy the agent with your curl in its `curl` slot:

```sh
kubectl apply -f steps/02-build-curl/workload-mine.yaml \
  && sleep 2 && kubectl wait --for=condition=Ready \
  workloaddeployment/toolbox-agent
```

`workload-mine.yaml` is `workload.yaml` with one change: the `curl` image is
`tool-curl:mine`. Built again? Run `make push-build`, then
`kubectl delete workloaddeployment toolbox-agent` and apply again, so the host
fetches the new push.

## Path B: use the prebuilt curl

Look inside the prebuilt tool:

```sh
wasm-tools component wit artifacts/tool_curl.wasm \
  | grep -E 'import wasi:(http|sockets|filesystem)'
```

Deploy:

```sh
kubectl apply -f steps/02-build-curl/workload.yaml \
  && sleep 2 && kubectl wait --for=condition=Ready \
  workloaddeployment/toolbox-agent
```

## What the imports say

Both builds print the same four lines:

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

The prebuilt tool answers `saved 95 bytes to /data/page.html`. Your build
answers with the message you wrote, which shows your code is the one running.

Step 3 goes back to the prebuilt curl and tries everywhere else. To keep your
own build in later steps, change the `curl` image in that step's manifest to
`tool-curl:mine`.

Full transcript (path B): [`expected-output.txt`](expected-output.txt).
