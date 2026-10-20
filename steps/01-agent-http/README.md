# Step 1: meet the agent

**Folder:** `steps/01-agent-http`

The agent here is four parts: an agent harness (the component that runs the
loop: send the conversation to the model, call the tool the model asks for,
send the result back, repeat), a model, tools, and the storage the tools use.
In this step you deploy the harness and the scripted model. The four tool
slots (`memory`, `notes`, `reports`, `web`) are all filled by `not-installed`,
one placeholder image whose every function answers "the <tool> tool isn't
installed yet".

Open `workload.yaml`. Each entry under `components:` is one component (a
small WebAssembly program, like a tiny container image that starts with no
access to anything). Notice what the harness and the model are given: no
`allowedHosts`, no volumes, no environment. Nothing.

**Run** (from the repo root, after `./scripts/up.sh` and `./scripts/push.sh artifacts`):

```sh
python3 scripts/apply.py steps/01-agent-http/workload.yaml
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d 'hello'
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d 'read my notes'
```

**Expected:**

```
default-toolbox-agent: WORKLOAD_STATE_RUNNING Workload started successfully

[user] hello
[model] Hello! I am the workshop agent running a scripted model. Ask me to remember something, read my notes, fetch a page, or save a report.

[user] read my notes
[model] call read_note({"path":"todo.txt"})
[tool read_note] error: the notes tool isn't installed yet
[model] Done. Here is what each step returned: read_note: ERROR: the notes tool isn't installed yet
```

**Look inside.** The harness's WIT (the list of what it imports and
exports) shows the model and four typed, labelled tool slots, and no
filesystem, outgoing HTTP or sockets:

```sh
wasm-tools component wit artifacts/agent.wasm | grep 'import .*workshop:toolbox'
```

```
  import workshop:toolbox/types@0.1.0;
  import model: workshop:toolbox/model@0.1.0;
  import memory: workshop:toolbox/memory@0.1.0;
  import notes: workshop:toolbox/notes@0.1.0;
  import reports: workshop:toolbox/reports@0.1.0;
  import web: workshop:toolbox/web@0.1.0;
```

Each label (`memory`, `notes`, ...) names the component in the workload that
fills it. Swapping a tool is a manifest change, not a rebuild.

Full transcript: `expected-output.txt`.
