# Step 6: save reports to object storage

**Folder:** `steps/06-reports`

You install the `reports` tool: `save(name, body)`. Its one capability is
blobstore (`wasmcloud:blobstore`), where an object's data travels as a stream.
As with memory, the platform declared the backend and created the store
(`reports`); the workload only asks for blobstore.

**Run:**

```sh
kubectl apply -f steps/06-reports/workload.yaml
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d 'what do you remember? Then save a report'
nats --server nats://127.0.0.1:14322 object get reports report.txt
```

**Expected:**

```
[tool recall_all] ok: my favorite color = teal; my api key = sk-demo-123
[tool save_report] ok: saved 91 bytes as report.txt
```

and the object store holds the report text. Durable state lives behind a
granted interface; the agent keeps nothing in memory between runs.

Full transcript: `expected-output.txt`.
