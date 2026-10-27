# Step 2: give the agent a memory

**Folder:** `steps/02-memory`

You install the `memory` tool. Its interface is ordinary and typed:

```wit
remember: async func(key: string, fact: string) -> result<_, string>;
recall: async func(key: string) -> result<option<string>, string>;
recall-all: async func() -> result<list<entry>, string>;
```

The tool knows nothing about agents or models. The model speaks JSON, so the
agent harness translates: `components/agent/src/tools.rs` has one row per
function, with the name and JSON Schema the model sees, and the typed call it
makes.

**The capability.** `memory` is the only component that imports keyvalue
(`wasmcloud:keyvalue/store`). In `workload.yaml` the workload asks for it
under `hostInterfaces`, and the memory component names its bucket in
`MEMORY_BUCKET`. That is all the workload says. Where the data lives is the
platform's decision: `k8s/values.yaml` declares the backend (the cluster's
data service) under `plugins:`, and `scripts/stores.sh` created the bucket.
A workload can open a bucket the platform made; it cannot create one, and it
cannot point keyvalue at an address of its own choosing.

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
kept in memory between calls. Remember the API key: step 7 tries to steal it.

```sh
nats --server nats://127.0.0.1:14322 kv ls agent-memory
```

Full transcript: `expected-output.txt`.
