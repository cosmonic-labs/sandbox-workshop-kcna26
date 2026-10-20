# Step 2: give the agent a memory

**Folder:** `steps/02-memory`

You install the `memory` tool. Its interface is ordinary and typed:

```wit
remember: func(key: string, fact: string) -> result<_, string>;
recall: func(key: string) -> result<option<string>, string>;
recall-all: func() -> result<list<entry>, string>;
```

The tool knows nothing about agents or models. The model speaks JSON, so the
agent harness translates: `components/agent/src/tools.rs` has one row per
function, with the name and JSON Schema the model sees and the typed call it
turns into. In an agent platform the host does this for you; here you can see
it.

The one capability `memory` gets is `wasi:keyvalue`, declared in
`hostInterfaces`, with the bucket named in its own environment
(`MEMORY_BUCKET`). Under `wash host` the bucket is NATS JetStream KV. No other
component in the workload imports keyvalue.

**Build it yourself** (optional; the prebuilt one is in `artifacts/`):

```sh
cargo build --release --target wasm32-wasip2 -p tool-memory
./scripts/push.sh
```

**Run:**

```sh
python3 scripts/apply.py steps/02-memory/workload.yaml
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d 'remember that my favorite color is teal'
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d 'remember that my api key is sk-demo-123'
python3 scripts/apply.py steps/02-memory/workload.yaml        # restart the workload
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d 'what do you remember?'
nats --server nats://127.0.0.1:14222 kv ls agent-memory
```

**Expected** (last request):

```
[user] what do you remember?
[model] call recall_all({})
[tool recall_all] ok: my favorite color = teal; my api key = sk-demo-123
[model] Done. Here is what each step returned: recall_all: my favorite color = teal; my api key = sk-demo-123

my_favorite_color
my_api_key
```

The API key matters later: in step 6 a planted page tries to steal it.

Full transcript: `expected-output.txt`.
