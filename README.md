# Sandboxing on Kubernetes with WebAssembly
## Multi-tenant sandboxing for agent harnesses and tools on wasmCloud

A hands-on workshop for KubeCon + CloudNativeCon NA 2026. You `kubectl apply`
an agent and four tools to your own cluster with **wasmCloud**, a CNCF
project, then try each tool's most famous escape and watch it fail.

The tools are `curl`, `sqlite3`, `git` and `kubectl`. The first three are classics from
[GTFOBins](https://gtfobins.github.io/), the catalogue of everyday programs
that break out of restricted shells. The model is a gullible stand-in that
does whatever it reads. It never refuses, so every refusal you see comes from
another layer, and each step names which one:

* **Imports**: what a component can ask the host for at all. None of these
  tools imports a way to start a process, and none imports sockets.
* **The tool's own code**: this git offers no `push`, and this sqlite3's
  small shell answers `.shell` itself and has no extension loading compiled
  in.
* **The grant**: the lines of the manifest that give each tool its folders
  (`volumeMounts`) and its hosts (`allowedHosts`).
* **RBAC** (role-based access control, the Kubernetes API server's own
  rules): what the identity behind `kubectl` may do (step 6).

Each tool starts with nothing and gets only its own grant.

## Requirements

* Docker (Docker Desktop, OrbStack or Podman) with 4 GB or more of memory for
  containers, on macOS or Linux. On Windows, use WSL2.
* Or nothing at all: GitHub Codespaces (see below).
* About 1 GB of downloads. Do them before the conference: `make tools && make up`.
* Optional, to build `curl` yourself in step 2: Rust 1.98.1 with the
  `wasm32-wasip2` target, and one `make build` beforehand to fetch the
  crates. Without it, step 2 uses the prebuilt image.

`make tools` downloads every other tool (wash, kind, kubectl, helm, nats,
wasm-tools) into `.tools/`, pinned and checksum checked. Nothing is installed
outside the repo and nothing needs sudo.

## Set up

```sh
git clone https://github.com/cosmonic-labs/agent-tools-workshop
cd agent-tools-workshop
make tools                         # download the pinned tools into .tools/
export PATH="$PWD/.tools:$PATH"    # use those tools in this terminal
make doctor                        # check this machine; changes nothing
make up                            # kind cluster and the wasmCloud chart, about 2 minutes the first time
```

Every command in the steps runs from the `agent-tools-workshop` folder. In
each new terminal, `cd` there and run the `export PATH` line again, so
`kubectl` and `wasm-tools` are the pinned ones. `make up` creates a kind
cluster named `workshop-toolbox` and switches your kubectl context to it.

## Steps

| Step | Folder | What you'll see |
| --- | --- | --- |
| 1 | `steps/01-run-it` | The agent and its tools deploy with `kubectl` and answer |
| 2 | `steps/02-build-curl` | `curl` imports only an HTTP client; its reach is one line of YAML |
| 3 | `steps/03-curl` | `curl`'s escapes (metadata service, other hosts, redirects, files) refused |
| 4 | `steps/04-sqlite` | `sqlite3`'s escapes (`.shell`, `ATTACH`, extensions) go nowhere |
| 5 | `steps/05-git` | A hostile repository names programs to run; native git runs them, the tool can't |
| 6 | `steps/06-kubectl` | Ordinary `kubectl` through a platform capability; RBAC still decides |

Steps 1 to 4 tell the whole story; 5 and 6 are for if time allows. Each step
README has the commands and the output to expect (`expected-output.txt`).

**Fell behind?** Every step's manifest is complete, so you can jump to any
step with its one `kubectl apply`.

## Chat in the browser, or use curl

On your own machine or in a local dev container, open
**http://agent.localhost:18180** in Chrome or Firefox to chat with the agent.
Browsers send `*.localhost` to your own machine, and the dev container
forwards port 18180, so it works in both. http://sites.localhost:18180 shows
live what has reached the toy attacker's server.

In Codespaces, use the curl command each step gives. Every request goes to
`127.0.0.1:18180` with a `Host` header naming the agent.

## GitHub Codespaces

No Docker? Open the repo in Codespaces (Code > Codespaces > Create codespace
on main) on a 4 core machine. Everything is installed, including Docker. Run
`make up` in its terminal and follow the steps with curl. A browser pointed at
the forwarded port shows nothing useful, because it sends the codespace's own
host name, not `agent.localhost`.

## Troubleshooting

| You see | Fix |
| --- | --- |
| `make doctor`: Docker is not running, or has under 4 GB | Start Docker, or raise its memory limit (Docker Desktop: Settings > Resources) |
| `make doctor`: Rust is not on your PATH | Only needed to build; run the `fix:` line it prints |
| `curl: (7) Failed to connect to 127.0.0.1 port 18180` | The cluster is not up: `make up` |
| `kubectl` talks to another cluster | `kubectl config use-context kind-workshop-toolbox` |
| `kubectl wait` times out | The host is still pulling: `kubectl get workloads -A`, then try again |
| Port 18180 is taken | `make clean`, then `HOST_HTTP=127.0.0.1:18190 make up` (the port is fixed when the cluster is created) |
| The collector count never changes | `scripts/stores.sh create` |

`make down` changes nothing. `make clean` deletes the cluster and its
registry.

## How it fits together

```
  curl or browser ──▶ 127.0.0.1:18180 ──▶ ingress (picks a Service by Host)
                                             │
            ┌────────────────────────────────┴───────────────┐
            ▼                                                ▼
  host pod, group "default"                       host pod, group "fixtures"
    workload toolbox-agent                          toy sites: api, collector,
      harness ──▶ model                             llm
        ├──▶ curl     allowedHosts only ─── HTTP ──▶ (.workshop-sites.svc)
        ├──▶ sqlite   /data, read only
        ├──▶ git      /repo, read only
        └──▶ kubectl  kube-api plugin ──▶ API server (RBAC: agent-reader)
```

* A **component** is a WebAssembly program that starts with no access to
  anything: no files, no network, no environment. Think of a very small
  container image.
* A **workload** is a group of components deployed together. You apply it as a
  `WorkloadDeployment`, which plays the role of a Deployment.
* A **host pod** runs the wasmCloud host, which runs many workloads and
  enforces what each component may reach. The runtime operator places
  workloads on host pods, like a Deployment controller.
* The **agent harness** runs the loop: ask the model, call the tool it picks,
  send back the result. It has no network or files of its own; each tool gets
  only what its entry in the manifest grants.

The toy sites (`steps/00-fixtures`) are fake websites inside the cluster, so
the workshop needs no internet.

## Known limits (wasmCloud v2.12.0)

* Workloads on one host are separated by the WebAssembly runtime alone. For
  mutually hostile tenants, use separate host groups or nodes, and decide who
  may apply workloads with RBAC and admission policy.
* `hostPaths.allowed` is host wide: any workload on a host may mount any
  folder in that list, so admission policy decides who may mount what.
* Concurrent linked calls can run with the wrong component's grants
  (wasmCloud#5756). The host tracks which component is active in a single
  slot, which is only correct when linked calls happen one after another. It
  can go wrong only when one store has several linked calls in flight at
  once: a harness that runs tool calls in parallel, or a pooled component
  with `maxConcurrency` above 1. The workshop's components aren't pooled (each
  request gets a fresh store) and the harness awaits each tool call before the
  next, so it can't happen here.
* The chart's NetworkPolicies are off in `k8s/values.yaml` because kindnet on
  kind v0.33.0 dropped their DNS replies. Keep them on in a real cluster.

## For maintainers

* `make build` builds `curl`; `make build-all` rebuilds every component and
  needs wasi-sdk (`WASI_SDK_PATH`, `CC_wasm32_wasip2`). `make push-build`
  pushes your build to the cluster's registry. A running workload keeps its
  old image until it is recreated.
* `make prove` replays every step and rewrites each `expected-output.txt`.
* `TESTING.md` is the pre-workshop test checklist.
* `extras/real-model/` swaps the scripted model for `model-real` (an OpenAI
  compatible endpoint) in the same slot, with no rebuild.
* The tool interfaces are in `wit/toolbox.wit`; the harness's tool table is
  `components/agent/src/tools.rs`.
