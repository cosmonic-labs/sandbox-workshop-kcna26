# Multi-tenant agents on Kubernetes: build your agent a toolbox

A hands-on workshop (KubeCon + CloudNativeCon NA 2026, 80 minutes) for
platform engineers: run AI agents on the Kubernetes cluster you already have,
many tenants to a pod, with **upstream wasmCloud**, a CNCF project.
Everything here is upstream: the wasmCloud v2.11.0 Helm chart and host,
standard WASI 0.3 interfaces, wasmCloud's own keyvalue interface, and one
small WIT package of our own (`workshop:toolbox`). No vendor products are
needed.

You `kubectl apply` a ready made agent and its three tools, then try the
tools' most famous escapes and watch each one fail. The tools are ordinary
developer programs compiled to components: `curl`, `sqlite3` and `git`. Each
is a classic from [GTFOBins](https://gtfobins.github.io/), the catalogue of
everyday tools that break out of restricted shells (`sqlite3`'s `.shell`,
`curl` reaching the cloud metadata service, `git`'s hooks). As components they
have nothing to escape to. You personalize `curl` and build it with `wash`,
then at the end swap the scripted model for a real one with no rebuild. Every
tool runs in its own sandbox with only the access you grant it. The model is a
scripted, gullible stand-in that runs whatever command it is told. **The point
is not that the model refuses; it does whatever it is asked. The sandbox
decides what the tool it is asking can reach.** The lesson is the one the
GTFOBins catalogue teaches in reverse: **the grant is the boundary, not the
command.**

## What you need

* Docker (Docker Desktop, OrbStack or Podman) with at least 4 GB of memory
  for containers, on macOS or Linux. On Windows, use WSL2.
* Or nothing at all: open the repo in GitHub Codespaces (Code > Codespaces >
  Create codespace on main), which has everything installed.
* About 1 GB of downloads, most of it kind's Kubernetes node image. Do them
  before the conference: `make tools && make up`.
* Optional, to build the curl tool yourself in step 2: Rust 1.98.1 with
  the `wasm32-wasip2` target (`make doctor` says whether you have it;
  Codespaces has it). Run `make build` once beforehand to fetch the crates.
  Without Rust, step 2 uses the prebuilt image. The `sqlite3` and `git` tools
  ship prebuilt in `artifacts/`, since rebuilding them needs a C toolchain
  (`make build-all`, for maintainers).

Every other tool (wash, kind, kubectl, helm, nats, wasm-tools) is downloaded
into `.tools/` by `make tools`, pinned and checked against a SHA256. Nothing
is installed outside the repo folder and nothing runs with sudo.

## Quick start

```sh
make tools     # download the pinned tools into .tools/
make doctor    # check this machine; changes nothing
make up        # kind cluster + the wasmCloud chart, about 2 minutes the first time
```

`make up` creates a one node kind cluster named `workshop-toolbox` and
switches your kubectl context to it (`kubectl config use-context <yours>`
switches back). Then follow `steps/01-run-it/README.md`. Each step has a
folder, the commands to run and the output to expect (`expected-output.txt`).

| Step | What you do | What it shows |
| --- | --- | --- |
| 1 | `kubectl apply` the agent and ask it something | A Workload is a custom resource; it runs on a wasmCloud host pod, and the tools are tiny |
| 2 | Build and personalize `curl` with `wash` | A component imports only what it asks for; `allowedHosts` is the grant |
| 3 | GTFOBins: `curl` tries the cloud metadata service, another host, a redirect, `-o` outside its folder | Every one refused at the grant |
| 4 | GTFOBins: `sqlite3` tries `.shell`, `ATTACH` outside its folder, `load_extension` | Nothing to escape to |
| 5 | GTFOBins: `git` reads a hostile repository (hooks, fsmonitor, pager, filters) | None of its traps fire |
| 6 | Swap in a real model | Same slot, new implementation, no rebuild |

Steps 1 to 4 tell the whole story. Steps 5 and 6 go as far as time allows.
After every `kubectl apply`, each step waits (`sleep 2 && kubectl wait ...`)
before sending a request, so the request reaches the new version.
Every step's manifest is complete, so you can jump to any step with one
`kubectl apply`. Two demos run from the presentation rather than your own
cluster: the same agent leaking its Secret and token from an ordinary
container (`extras/container/`), and scaling to a large number of workloads.

Two optional browser pages help on a projector: http://agent.localhost:18180
sends prompts and draws each transcript as a timeline, and
http://sites.localhost:18180 shows live what has reached the attacker. Your
browser sends `*.localhost` to your own machine, so they work locally, not in
Codespaces. curl stays the way every step is written.

`make prove` replays every step and rewrites the expected outputs, plus a
few extra checks no step asks you to run (`steps/*/control-checks.txt`). `make
down` does nothing destructive; `make clean` deletes the cluster.

## The architecture: one box

```
                         kind cluster ("your cluster")
  curl ──▶ 127.0.0.1:18180 ──▶ workshop-ingress (nginx): picks a Service by Host
                                  │ agent.localhost, a.agent...     │ sites.localhost
   namespace wasmcloud            ▼                                 │
   ┌──────────────────┐  ┌──────────────────────────┐  namespace    │ workshop-sites
   │ runtime-operator │─▶│ host pod, group default  │  ┌────────────▼──────────────┐
   │ NATS (control)   │  │   default: toolbox-agent │  │ host pod, group fixtures  │
   └──────────────────┘  │     curl, sqlite, git    │  │   toy-sites               │
                         └──┬────────────────────┬──┘  └───────────────────────────┘
   namespace                │ keyvalue           │ HTTP  ▲ Services api, planted,
   workshop-data            │ (toy sites only)   │       │ collector, llm
   ┌──────────────────┐     │                    └───────┘ (.workshop-sites.svc)
   │ data-nats        │◀────┘                      only hosts in a component's
   └──────────────────┘                            allowedHosts; all else denied
```

* **One way in.** A small nginx behind a NodePort (`k8s/ingress.yaml`)
  sends each agent's hostname to that agent's Service and `sites.localhost`
  to the fixtures host group, the job an ingress controller or Gateway API
  does in a real cluster. kind maps the node port to `127.0.0.1:18180`. An
  agent's Service has no selector: the runtime operator keeps its endpoints
  pointed at the host pods that run the agent (`spec.kubernetes.service`), so
  a request never lands on a host pod without it. Inside a host pod, the host
  sends each request to a workload by its `Host` header.
* **The control plane.** The runtime operator watches Workload resources and
  places them on hosts, like a Deployment controller; it talks to the hosts
  over NATS (with mutual TLS).
* **Host pods.** Each host pod runs many workloads from many namespaces. A
  workload is a group of components with its own grants.
  There are two host groups: `default` for the teams' agents, and
  `fixtures`, in its own namespace, for the toy sites. A workload picks one
  with `hostSelector: {hostgroup: ...}`.
* **The data service.** The toy sites' collector keeps its count in keyvalue.
  The platform declares where keyvalue data lives (`k8s/values.yaml`,
  `plugins:`) and creates the bucket (`scripts/stores.sh`). A workload only
  asks for "keyvalue": since v2.11.0 it may not name a backend's address
  itself, and it can open a bucket but not create one. The agents' three tools
  reach no keyvalue store at all.
* **Only granted ways out.** A component reaches a host over HTTP only if
  that host is in its own `allowedHosts`. Everything else is denied by
  default. The toy sites are ordinary Kubernetes Services, so an agent's
  request to `planted.workshop-sites.svc` leaves its host pod and crosses the
  cluster network like any other.

## Words we use

If you know Kubernetes, most of this maps onto things you already know.

* **Agent**: the whole thing that takes a request and does work for you. Here
  an agent is four parts: an agent harness, a model, a set of tools, and the
  storage those tools use.
* **Agent harness**: the part of the agent that runs the loop. It sends the
  conversation to the model, and when the model asks for a tool, it calls the
  tool and sends the result back, repeating until the model answers in plain
  text. Ours is prebuilt; you never have to rebuild it.
* **Model**: the language model that decides what to do next. Here it is
  either a scripted fake (`model-scripted`) or a client for an OpenAI
  compatible API (`model-real`).
* **Tool**: a small program the model can ask the harness to run, such as
  `curl`, `sqlite3` or `git`.
* **Component**: a WebAssembly program packaged with a description of what it
  needs and what it offers. Think of it as a very small container image that
  starts with no access to anything: no files, no network, no environment
  variables unless granted. Components are pushed to and pulled from an OCI
  registry, like container images.
* **Host**: the wasmCloud runtime. On Kubernetes it runs as a pod (a "host
  pod"), a little like a kubelet plus a container runtime that lives inside
  one pod. It links components together and enforces what each may reach.
* **Workload**: a group of components deployed together, described in one
  YAML manifest. You apply it as a `WorkloadDeployment`, which plays the role
  of a Deployment; the operator schedules it onto a host pod.
* **Capability**: one specific kind of access a component is granted, such as
  a keyvalue bucket, one folder, or a list of hosts it may call over HTTP.
  Capabilities are granted per component in the manifest, much like a volume
  mount or a network policy, except that the default is "nothing".
* **WIT**: the interface definition language for components (in the same
  family as protobuf or OpenAPI). A WIT file lists the functions a component
  exports and the functions it imports. If an import is not in the WIT, the
  component cannot ask for it.

## The design

### One agent, one component per tool

```
                      workload "toolbox-agent"
  HTTP  ┌──────────────────────────────────────────────────────────────┐
 ─────▶ │  agent (harness) ──▶ model                                   │
        │        │                                                     │
        │        ├──▶ curl     ── HTTP out       (allowedHosts list)   │
        │        ├──▶ sqlite   ── one folder, a database, read only    │
        │        └──▶ git      ── one folder, a repository, read only  │
        └──────────────────────────────────────────────────────────────┘
```

The harness imports the model and three tool slots and nothing else from the
host: no filesystem, no outgoing HTTP, no sockets. Each tool is a separate
component with its own capabilities. If the model is talked into misusing a
tool, the damage is limited to what that one tool was granted.

### The tools are ordinary typed interfaces

Each tool exports a plain interface, the kind you would write for any
program. None of them knows anything about agents, models or JSON. They
follow typed-tool conventions, the shape a host needs to offer a function to
a model:

- Each function takes one record, named `<function>-args`, whose fields are
  the tool's arguments. WIT has no doc comments on parameters, but it has
  them on record fields, so each argument gets a description.
- Each interface has its own `variant error`: a fixed list of the ways the
  tool can fail (`not-found`, `access-denied` and so on), ending in `other`.
  A caller, or a model, can tell a missing file from a refused one without
  reading a message.
- Every interface, type, case, field and function has a doc comment,
  written for the model as much as for a programmer.

Each of the three tools is a familiar command line program, so its one
argument is the command line you would type after the program's name. That
way an attendee teaches a tool new tricks without changing the WIT. From
`wit/toolbox.wit`, with the doc comments trimmed:

```wit
interface curl {
  variant error {
    invalid-argument(string), access-denied(string),
    network(string), other(option<string>),
  }
  record run-args { arguments: string }
  run: async func(args: run-args) -> result<string, error>;
}

interface sqlite {
  variant error {
    invalid-argument(string), access-denied(string),
    not-found(string), other(option<string>),
  }
  record query-args { sql: string }
  query: async func(args: query-args) -> result<string, error>;
}

interface git {
  variant error {
    invalid-argument(string), access-denied(string),
    not-found(string), other(option<string>),
  }
  record run-args { arguments: string }
  run: async func(args: run-args) -> result<string, error>;
}

/// The agent harness: one model and one labelled slot per tool, and HTTP in.
world agent {
  import model: model;
  import curl: curl;
  import sqlite: sqlite;
  import git: git;
  export wasi:http/handler@0.3.0;
}
```

The functions are `async` because everything here is WASI 0.3, where a call
into another component, a database read or an HTTP request can wait without
blocking the host.

### The translation table: what a host does for you, by hand

A model only speaks JSON. It is shown a list of tools, each with a name, a
description and a JSON Schema, and it answers with something like
`{"name": "curl", "arguments": {"arguments": "-s https://example.com"}}`. The
tools only speak typed functions. Something has to translate between the two.

A host could do that for you. It could read each tool's WIT and build the
definitions the model sees from the names, types and doc comments: the
function's doc becomes the tool's description, and the record's fields
become its arguments. It would turn the model's JSON into a typed call, and
the typed result or error back into text. Upstream wasmCloud doesn't do that
yet, so this workshop's agent harness does it by hand, in plain sight, in
`components/agent/src/tools.rs`: one row per function, copied from the WIT,
and one `match` arm that builds the args record and makes the typed call.

```rust
"curl" => {
    let args = curl::RunArgs { arguments: a.get("arguments")? };
    curl::run(args).await.map_err(curl_error)
}
```

A typed error reaches the model as the JSON form of the WIT variant, the same
way a host renders it: `{"access-denied":"169.254.169.254: this tool may not
reach that host (not in its allowedHosts)"}`. The rows give each tool a name
that reads well to a model (`curl` for the curl tool's `run`), the way a host
keeps a rename table.

### Labelled slots

`import curl: curl;` is a labelled import (wasmCloud#5593). The host links
the label to the component with the same `name` in the workload, with no
extra composition step. So the manifest, not the harness code, decides which
component sits behind each slot, and each tool keeps its own capabilities.
Each tool interface defines its own types, so a slot brings everything its
functions need with it.

Every label must be filled or the workload will not start. A slot with no real
tool yet is filled by `not-installed`: one image that exports all three tool
interfaces, whose every function returns its interface's `other` error, `the
<tool> tool isn't installed yet`. Labels are fixed when the harness is built,
so adding a fourth tool means a rebuild; upstream has no dynamic tool discovery
today.

### The gullible model

`model-scripted` plans tool calls from the user prompt **and every tool
result**, in order, and runs each instruction once. It passes a tool's own
command line straight through, and maps a few plainer requests onto a tool:

| It reads | It calls |
| --- | --- |
| `curl <args>` | `curl {arguments: <args>}` |
| `sqlite <sql>` / `sql <sql>` / a line starting with `.` | `sqlite {sql}` |
| `git <args>` | `git {arguments: <args>}` |
| `fetch` / `get` / `summarize <url>` | `curl {arguments: "-s <url>"}` |
| `save <url> to <file>` | `curl {arguments: "-o <file> -s <url>"}` |
| `list the talks` / `what talks` | `sqlite {sql: "select ... from talks ..."}` |
| `show the log` / `recent commits` | `git {arguments: "log --oneline -5"}` |

It needs no API key, no GPU and gives the same result on every laptop. It does
whatever it is asked, so the only thing that stops a dangerous command is the
sandbox around the tool that runs it.

### The toy sites

`steps/00-fixtures/` runs one component that serves several fake sites inside
the cluster, started by `make up`, so the workshop needs no internet:

* `api.workshop-sites.svc/data`: an ordinary page, the one host the `curl`
  tool is granted. `/redirect` sends the caller to the collector, a host
  `curl` is not granted, so step 3 can show the host checks each hop.
* `collector.workshop-sites.svc/collect`: a stand-in for an attacker's
  server; it counts every request that reaches it.
* `planted.workshop-sites.svc`: a page with a hidden instruction, used by the
  container demo in the presentation.
* `llm.workshop-sites.svc`: a fake OpenAI compatible model for step 6.

Each name is a Kubernetes Service (`k8s/sites.yaml`) in front of the fixtures
host group, which runs nothing else; nothing leaves your machine. `curl -H
'Host: sites.localhost' 127.0.0.1:18180/count` shows the collector's count,
and a POST to `/reset` zeroes it. Only the name `sites.localhost` may reset or
list the leaks. In a browser, http://sites.localhost:18180 shows each one as
it arrives.

## WASI 0.3

Every capability the workshop teaches is WASI 0.3: HTTP in and out
(`wasi:http@0.3.0`) and the filesystem. The agent harness, models and curl
tool are written against WASI 0.3; the `sqlite3` and `git` tools read files
through Rust's standard library and C, which on `wasm32-wasip2` import
`wasi:filesystem@0.2`, so a tool carries both 0.2 and 0.3 imports and the host
links each. The toy sites' collector uses wasmCloud's async
`wasmcloud:keyvalue@0.2.0`. The components are built with `wit-bindgen` 0.62
for the `wasm32-wasip2` target. `wasm-tools component wit
artifacts/<name>.wasm` lists exactly what each component imports.

## Building the components yourself

The prebuilt components in `artifacts/` (checked by `artifacts/SHA256SUMS`)
are what `make up` pushes. To build the curl tool yourself:

```sh
make build        # the curl tool; needs Rust 1.98.1 with the wasm32-wasip2 target
make push-build   # push target/wasm32-wasip2/release/*.wasm to the cluster's registry
```

`make build` builds only `curl`, the tool step 2 asks you to personalize. It
is pure Rust, so it needs no C toolchain, and the build is fast once the crates
are fetched. The `sqlite3` and `git` tools ship prebuilt in `artifacts/`,
because `sqlite3` bundles SQLite's C source and so needs wasi-sdk; `make
build-all` rebuilds everything and expects `WASI_SDK_PATH` and
`CC_wasm32_wasip2` set. The fixtures (the talks database, the hostile
repository) are rebuilt from `fixtures/` each `make up`, so nothing binary is
committed.

`make push-build` pushes under the same tags as the prebuilt images, and a
host pulls an image only when a workload starts. A workload that's already
running keeps its old image until it's recreated (`kubectl delete -f` and
`kubectl apply -f`); `make up` recreates the toy sites for this reason.

## No Docker? GitHub Codespaces

The devcontainer installs everything, including Docker, so the cluster runs
inside the codespace. Pick a 4 core machine. Open a terminal and run `make
up`; the step commands work unchanged, because they run in the codespace's
own terminal against `127.0.0.1:18180`. Opening port 18180 in a browser
shows nothing useful, because the browser sends the codespace's own host
name, not `agent.localhost`. Use `curl`.

## Troubleshooting

| You see | Why | Fix |
| --- | --- | --- |
| `make doctor`: Docker is not running | kind needs it | Start Docker, wait until it says it is running |
| `make doctor`: Docker has under 4 GB | The cluster will not fit | Raise Docker's memory limit (Docker Desktop: Settings > Resources) |
| `curl: (7) Failed to connect to 127.0.0.1 port 18180` | The cluster is not up | `make up` |
| `kubectl` talks to another cluster | Your context changed back | `kubectl config use-context kind-workshop-toolbox` |
| `kubectl wait` times out | The host is still pulling | `kubectl get workloads -A`, then try again; `make up` reloads everything |
| The collector count never changes | The store was reset or never made | `scripts/stores.sh create` |
| Port 18180 is taken | Another program has it | `HOST_HTTP=127.0.0.1:18190 make up` after `make clean` (the port is fixed when the cluster is created) |

## Layout

```
artifacts/        prebuilt components and SHA256SUMS (what make up pushes)
components/       the Rust source: agent, models, tools, not-installed, toy-sites
  common/         HTTP helpers over wasi:http@0.3.0, shared by source path
  tool-git/vendor two gitoxide crates patched for wasm (memmap2, filetime)
extras/container/ the gullible agent as a plain container (run from the talk)
fixtures/         the talks database (talks.sql) and hostile repository, as scripts
k8s/              chart values, the data service, the toy sites' Services, the ingress
scripts/          up, down, stores, prove, push, tools, doctor, fixtures
steps/            one folder per step: manifest, README, expected output
wit/              workshop:toolbox and the WASI 0.3 / wasmCloud WIT it uses
```

## What wasmCloud v2.11.0 does and does not give you here

Verified on 2026-11-03 on macOS arm64, kind v0.33.0 and the upstream chart
v2.11.0 (`make prove`).

* **Works as the steps claim:** labelled imports between components in one
  workload; per component `allowedHosts`, checked on every hop of a redirect;
  read only `hostPath` volumes on one component, inside the host's
  `hostPaths.allowed` list; a component that mixes WASI 0.3 exports with WASI
  0.2 filesystem imports (the `sqlite3` and `git` tools); keyvalue on a
  backend the platform declares; `kubectl scale` on a WorkloadDeployment.
* **A grant belongs to one component, for calls made one after another.**
  Concurrent calls through linked components can currently borrow a
  sibling's grants (wasmCloud#5756). The workshop's harness calls its tools
  one at a time.
* **`hostPaths.allowed` is host wide.** Any workload on a host may mount any
  directory in that list (wasmCloud's THREAT_MODEL.md: "Every workload on a
  host may mount any directory the operator allowed"), so admission policy
  decides who may mount what. The `sqlite3` and `git` tools mount their
  folders read only.
* **One isolation layer between tenants on a host.** Upstream's threat model
  is explicit: workloads on one host are separated by the WebAssembly runtime
  alone. For mutually hostile tenants, use separate host groups or nodes, and
  decide who may apply Workloads with RBAC and admission policy (wasmCloud
  ships no admission controller). The workshop keeps the toy sites on a host
  group of their own, and keeps local routing (`localBypassRouting`) off on
  every host group so no workload can claim another's hostname in memory.
* **Service names are listed by hand.** The operator adds a Service's DNS
  names to a workload's routes by itself only for wasi:http 0.2 handlers
  (`kubernetes.service`, wasmCloud#5582). The toy sites are WASI 0.3, so
  `toy-sites.yaml` lists them in `host-aliases`. `allowedHosts` matches the
  name exactly: a component allowed `planted.workshop-sites.svc` may not
  call `planted.workshop-sites.svc.cluster.local`.
* **`--allow-insecure-registries`** turns off TLS for every registry the host
  pulls from. The workshop's registry is plain HTTP on the kind network; the
  chart's `ociCaPaths` is the safer choice for a real registry.
* **The chart's NetworkPolicies are off in `k8s/values.yaml`.** On kind
  v0.33.0, kindnet's policy enforcement dropped the DNS replies to the
  operator and host pods, which then crash looped. Keep them on in a real
  cluster.
* **The workshop runs its own data service** because the chart's NATS
  requires mutual TLS with client certificates, which the multiplexed
  keyvalue backend does not present.
