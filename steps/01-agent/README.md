# Step 1: deploy the agent with kubectl

**Folder:** `steps/01-agent`

The agent here is four parts: an agent harness (the component that runs the
loop: send the conversation to the model, call the tool the model asks for,
send the result back, repeat), a model, tools, and the storage the tools use.
In this step you deploy the harness and the scripted model. The three tool
slots (`memory`, `notes`, `web`) are all filled by `not-installed`,
one placeholder image whose every function answers "the <tool> tool isn't
installed yet".

Open `workload.yaml`. It is a `WorkloadDeployment`, a custom resource that
plays the role of a Deployment. Each entry under `components:` is one
component (a small WebAssembly program, like a tiny container image that
starts with no access to anything). Notice what the harness and the model are
given: no `allowedHosts`, no volumes, no environment. Nothing.

**Run** (from the repo root, after `make up`; in a new terminal, first run
`export PATH="$PWD/.tools:$PATH"` so `kubectl` is the pinned one):

```sh
kubectl apply -f steps/01-agent/workload.yaml
kubectl wait --for=condition=Ready workloaddeployment/toolbox-agent
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d 'hello'
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d 'read my notes'
```

**Expected:**

```
[user] hello
[model] Hello! I am the workshop agent running a scripted model. Ask me to remember something, read my notes, or fetch a page.

[user] read my notes
[model] call read_note({"path":"todo.txt"})
[tool read_note] error: {"other":"the notes tool isn't installed yet"}
[model] Done. Here is what each step returned: read_note: ERROR: {"other":"the notes tool isn't installed yet"}
```

Optional: on your own machine you can also open http://agent.localhost:18180
in a browser, which sends prompts the same way and draws each transcript as a
timeline. In Codespaces, use the curl commands.

The error is typed. Each tool interface has its own `variant error`, a fixed
list of the ways it can fail, and the harness shows the model which case it
got as `{"case": "payload"}`. The placeholder answers with `other`.

**Where it runs.** The operator placed the workload on the one wasmCloud host
pod in the cluster:

```sh
kubectl get workloads -A -o custom-columns=NAMESPACE:.metadata.namespace,HOST:.status.hostId
kubectl get pods -n wasmcloud -l wasmcloud.com/name=hostgroup
```

You see two workloads (your agent and the toy sites) on one host id, and one
host pod.

**Look inside.** The harness's WIT (the list of what it imports and exports)
shows the model and three typed, labelled tool slots, and no filesystem,
outgoing HTTP or sockets:

```sh
wasm-tools component wit artifacts/agent.wasm | grep 'import .*workshop:toolbox'
```

```
  import workshop:toolbox/types@0.4.0;
  import model: workshop:toolbox/model@0.4.0;
  import memory: workshop:toolbox/memory@0.4.0;
  import notes: workshop:toolbox/notes@0.4.0;
  import web: workshop:toolbox/web@0.4.0;
```

Each label (`memory`, `notes`, ...) names the component in the workload that
fills it. Swapping a tool is a manifest change, not a rebuild.

Full transcript: `expected-output.txt`.
