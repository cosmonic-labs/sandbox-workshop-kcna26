# Step 2: build the agent a memory

**Folder:** `steps/02-memory`

You build and install the `memory` tool, the first tool in the agent's
toolbox. Its interface is ordinary and typed, with
doc comments the model ends up reading (trimmed here):

```wit
/// Arguments for `remember`.
record remember-args {
  /// A short name for the fact, such as "favorite color".
  key: string,
  /// The fact itself.
  fact: string,
}

/// Remember a fact under a short key, for later conversations. A fact
/// already stored under the key is replaced.
remember: async func(args: remember-args) -> result<_, error>;
recall: async func(args: recall-args) -> result<option<string>, error>;
recall-all: async func() -> result<list<entry>, error>;
```

Each function takes one record, so every argument has a doc comment, and
fails with the interface's own `error` variant: `no-such-store` when the
store is missing, `access-denied` when the store refuses, and so on.

The tool knows nothing about agents or models. The model speaks JSON, so
something has to translate. A host could build these tool definitions from
the WIT and its doc comments; upstream wasmCloud doesn't do that yet, so this
workshop's harness does it by hand: `components/agent/src/tools.rs` has one row per function, with the
name, description and JSON Schema the model sees, copied from the WIT, and
the typed call it makes.

**The capability.** `memory` is the only component that imports keyvalue
(`wasmcloud:keyvalue/store`). In `workload.yaml` the workload asks for it
under `hostInterfaces`. That is all the workload says. Where the data lives
is the platform's decision: `k8s/values.yaml` declares the backend (the
cluster's Valkey data service) under `plugins:`, and the key prefix this
workload's store gets (`agent:`). A workload cannot point keyvalue at an
address of its own choosing or pick its own prefix: since v2.11.0 the host
refuses a manifest that tries. Blobstore (object storage) works the same
way: the platform declares the backend, and the workload only asks for the
interface.

## Build it

The tool is one Rust file, `components/tool-memory/src/lib.rs`, about 110
lines. Open it: it imports nothing but keyvalue (`wasmcloud::keyvalue::store`).
The three `async fn`s under `impl Guest for Memory` are the WIT functions
above. (The `cfg(feature = ...)` lines at the top build the same file for
step 3's teams; you can ignore them here.)

Building needs Rust 1.98.1 with the `wasm32-wasip2` target (`make doctor`
says whether you have it). No Rust? Skip to **Run**: the prebuilt image is
the same tool.

```sh
export PATH="$PWD/.tools:$PATH"          # the pinned wash, kubectl and wasm-tools
wash -C components/tool-memory build
wash inspect target/wasm32-wasip2/release/tool_memory.wasm
```

`wash build` runs the build command in the tool's `.wash/config.yaml`
(`cargo build --release --target wasm32-wasip2`) and prints where the
component landed. `wash inspect` shows its imports and exports, the whole
list of what it can ask the host for:

```
world root {
  import wasmcloud:keyvalue/types@0.2.0;
  import wasmcloud:keyvalue/store@0.2.0;
  ...
  export workshop:toolbox/memory@0.4.0;
}
```

(The `@0.2.9` lines in between are the basics Rust's standard library links
for the `wasm32-wasip2` target: stdout, stderr, environment, exit.) There is
no filesystem, no socket and no HTTP client in the list.

Push it to the cluster's registry under a tag of your own, and deploy step
2's manifest with your image in the memory slot:

```sh
TAG=mine-$(date +%s)
wash oci push --insecure localhost:15052/workshop/tool-memory:$TAG target/wasm32-wasip2/release/tool_memory.wasm
sed "s#workshop/tool-memory:0.4.0#workshop/tool-memory:$TAG#" steps/02-memory/workload.yaml | kubectl apply -f -
sleep 2 && kubectl wait --for=condition=Ready workloaddeployment/toolbox-agent
kubectl get workloaddeployment toolbox-agent -o jsonpath='{.spec.template.spec.components[2].image}'; echo
```

The last line prints your image, `workshop-toolbox-registry:5000/workshop/tool-memory:mine-...`.
A fresh tag each time matters: a host pulls an image when a workload starts,
so pushing a new build under a tag that's already running changes nothing.

**Make it yours.** In `recall_all`, sort the keys before reading them:

```rust
        let mut keys = b.list_keys(None, None).await.map_err(store_error)?.keys;
        keys.sort();
```

Build, push and deploy again with the same commands (a new `TAG`), then
give your build something to sort:

```sh
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d 'remember that my favorite color is teal'
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d 'remember that my api key is sk-demo-123'
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d 'what do you remember?'
```

```
[tool recall_all] ok: my api key = sk-demo-123; my favorite color = teal
```

Alphabetical: your build is the one answering. The prebuilt tool lists them
in the order the store returns them.

## Run

Tell the agent two things, redeploy it, and ask what it remembers. If you
built your own tool, the redeploy below switches to the prebuilt image, and
the memories are still there: they live in the platform's data service, not
in either build.

```sh
kubectl apply -f steps/02-memory/workload.yaml && sleep 2 && kubectl wait --for=condition=Ready workloaddeployment/toolbox-agent
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d 'remember that my favorite color is teal'
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d 'remember that my api key is sk-demo-123'
kubectl delete -f steps/02-memory/workload.yaml && kubectl apply -f steps/02-memory/workload.yaml
sleep 2 && kubectl wait --for=condition=Ready workloaddeployment/toolbox-agent
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d 'what do you remember?'
```

**Expected** (the last command, after the redeploy):

```
[user] what do you remember?
[model] call recall_all({})
[tool recall_all] ok: my favorite color = teal; my api key = sk-demo-123
```

The memory survives the redeploy because it lives in the data service, not
in the component. Every call to a tool runs in a fresh instance; nothing is
kept in memory between calls. Remember the API key: step 5 tries to steal it.

You can look at the store directly, with `valkey-cli` in the data service's
pod. Every key has this workload's store prefix, `agent:`:

```sh
kubectl exec -n workshop-data deploy/valkey -- valkey-cli --scan --pattern 'agent:*'
```

Memories from an earlier run stay in the store, so a step that failed can
still show old facts. `scripts/stores.sh reset` empties every store.

Full transcript: `expected-output.txt`.
