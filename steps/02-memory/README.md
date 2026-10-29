# Step 2: give the agent a memory

**Folder:** `steps/02-memory`

You install the `memory` tool. Its interface is ordinary and typed, with
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
bucket is missing, `access-denied` when the store refuses, and so on.

The tool knows nothing about agents or models. The model speaks JSON, so
something has to translate. A host could build these tool definitions from
the WIT and its doc comments; upstream wasmCloud doesn't do that yet, so this
workshop's harness does it by hand: `components/agent/src/tools.rs` has one row per function, with the
name, description and JSON Schema the model sees, copied from the WIT, and
the typed call it makes.

**The capability.** `memory` is the only component that imports keyvalue
(`wasmcloud:keyvalue/store`). In `workload.yaml` the workload asks for it
under `hostInterfaces`, and the memory component names its bucket in
`MEMORY_BUCKET`. That is all the workload says. Where the data lives is the
platform's decision: `k8s/values.yaml` declares the backend (the cluster's
data service) under `plugins:`, and `scripts/stores.sh` created the bucket.
A workload can open a bucket the platform made; it cannot create one, and it
cannot point keyvalue at an address of its own choosing. Blobstore (object
storage) works the same way: the platform declares the backend and creates
the store, and the workload only asks for the interface.

**Run:**

```sh
kubectl apply -f steps/02-memory/workload.yaml
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d 'remember that my favorite color is teal'
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d 'remember that my api key is sk-demo-123'
kubectl delete -f steps/02-memory/workload.yaml && kubectl apply -f steps/02-memory/workload.yaml
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

```sh
nats --server nats://127.0.0.1:14322 kv ls agent-memory
```

Full transcript: `expected-output.txt`.
