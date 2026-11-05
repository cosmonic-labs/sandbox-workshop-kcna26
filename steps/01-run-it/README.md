# Step 1: Just run it!

**Folder:** `steps/01-run-it`

Deploy an agent and its tools with `kubectl`, then ask it questions. curl,
sqlite and git are ready, so the agent answers right away; the kubectl slot
holds a placeholder until step 6.

## Deploy

```sh
kubectl apply -f steps/01-run-it/workload.yaml \
  && sleep 2 && kubectl wait --for=condition=Ready \
  workloaddeployment/toolbox-agent
```

`sleep 2` gives the operator a moment to see the change. Without it,
`kubectl wait` can return before the new version is up.

## Chat with the agent

Open **http://agent.localhost:18180** in your browser and send each prompt
below. In Codespaces, use the curl command under each prompt instead.

**1. Ask sqlite:** `list the talks in sqlite`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'list the talks in sqlite'
```

The talks are made up. The database is mounted read only at `/data`.

**2. Ask git:** `show the recent commits`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'show the recent commits'
```

The repository is mounted read only at `/repo`.

**3. Ask curl:** `fetch http://api.workshop-sites.svc/data`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'fetch http://api.workshop-sites.svc/data'
```

That host is a toy site inside the cluster, and it is the only host curl is
granted.

Each answer is the whole transcript, so you can see the model call a tool and
what the tool returned.

## What you ran

One workload with six components: the agent harness (the loop that asks the
model, calls a tool and repeats), the model, and four tool slots: three real
tools and a placeholder in the kubectl slot. Each tool gets only what its entry in the manifest grants:

| Component | Can reach | Everything else |
| --- | --- | --- |
| harness, model | the tools in this workload | no network, no files |
| `sqlite` | `/data`, read only (the talks database) | refused |
| `curl` | `api.workshop-sites.svc`, and `/data` for `-o` | refused |
| `git` | `/repo`, read only | refused |
| `kubectl` (placeholder) | nothing; it answers "not installed yet" | refused |

The grants are a few lines of `workload.yaml`:

```yaml
- name: curl
  localResources:
    allowedHosts: [api.workshop-sites.svc]
    volumeMounts: [{ name: curl-data, mountPath: /data }]
- name: sqlite
  localResources:
    volumeMounts: [{ name: sqlite-data, mountPath: /data, readOnly: true }]
- name: git
  localResources:
    volumeMounts: [{ name: repo, mountPath: /repo, readOnly: true }]
```

The harness and the model have no `localResources` at all.

## Look inside (optional)

The tools are whole programs, and small:

```sh
ls -l artifacts/tool_curl.wasm artifacts/tool_sqlite.wasm artifacts/tool_git.wasm artifacts/tool_kubectl.wasm
```

`curl` is about 180 KB (182,276 bytes), `sqlite3` about 1.2 MB (1,173,947),
`git` about 2.5 MB (2,450,652) and `kubectl` about 800 KB (800,595). These
are the component files only; the wasmCloud host that runs them is a separate,
long-running pod.

The harness imports the model and four tool slots, and no network or files of
its own. The second command prints nothing:

```sh
wasm-tools component wit artifacts/agent.wasm | grep 'import .*workshop:toolbox'
wasm-tools component wit artifacts/agent.wasm | grep -E 'wasi:(http/client|filesystem|sockets)'
```

Full transcript: [`expected-output.txt`](expected-output.txt).
