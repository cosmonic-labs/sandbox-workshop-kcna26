# Step 1: run it

**Folder:** `steps/01-run-it`

Deploy the agent and its three tools, then ask it something. The agent is a
**Workload**, a custom resource the runtime operator schedules onto a wasmCloud
host pod. Every tool slot is already filled with a real tool, so the agent
answers straight away.

**Run:**

```sh
kubectl apply -f steps/01-run-it/workload.yaml && sleep 2 && kubectl wait --for=condition=Ready workloaddeployment/toolbox-agent
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d 'list the talks'
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d 'show the recent commits'
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d 'fetch http://api.workshop-sites.svc/data'
```

The first asks `sqlite` for rows from the talks database, the second asks `git`
for the log, the third asks `curl` for the one host it is granted. The response
is the whole transcript, so you can see the model call each tool.

**Look at the sizes.** These are whole programs, sandboxed:

```sh
ls -l artifacts/tool_curl.wasm artifacts/tool_sqlite.wasm artifacts/tool_git.wasm
```

`curl` is about 180 KB, `sqlite3` about 1.2 MB, `git` about 2.4 MB.

**The harness holds nothing.** It imports the model and three labelled tool
slots, and no HTTP client, filesystem or sockets of its own:

```sh
wasm-tools component wit artifacts/agent.wasm | grep 'import .*workshop:toolbox'
wasm-tools component wit artifacts/agent.wasm | grep -E 'wasi:(http/client|filesystem|sockets)'
```

The first prints the model and the `curl`, `sqlite` and `git` slots. The second
prints nothing. Whatever each tool can reach is decided by that tool's entry in
the manifest, not by the harness.

Full transcript: `expected-output.txt`.
