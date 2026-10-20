# Step 4: save reports to object storage

**Folder:** `steps/04-reports`

You install the `reports` tool: `save(name, body)`. Its one capability is
`wasi:blobstore`. Under `wash host` the backend is a NATS JetStream object
store. The stock host has no S3 backend, so this step is "object storage",
not "S3".

The NATS blobstore refuses (`unauthorized`) any container that is not listed
in the workload's `config.buckets`, so the manifest lists `reports`.

**Run:**

```sh
python3 scripts/apply.py steps/04-reports/workload.yaml
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d 'what do you remember? Then save a report'
nats --server nats://127.0.0.1:14222 object get reports report.txt -O .run/report.txt --force && cat .run/report.txt
```

**Expected:**

```
[model] call recall_all({})
[tool recall_all] ok: my favorite color = teal; my api key = sk-demo-123
[model] call save_report({"body":"what do you remember? Then save a report\nmy favorite color = teal; my api key = sk-demo-123","name":"report.txt"})
[tool save_report] ok: saved 91 bytes as report.txt

what do you remember? Then save a report
my favorite color = teal; my api key = sk-demo-123
```

Full transcript: `expected-output.txt`.
