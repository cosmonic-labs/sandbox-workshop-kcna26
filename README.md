# Multi-tenant agents on Kubernetes: build your agent a toolbox

A hands-on workshop (KubeCon + CloudNativeCon NA 2026, 80 minutes) for
platform engineers: run AI agents on the Kubernetes cluster you already have,
many tenants to a pod, with **upstream wasmCloud**, a CNCF project.
Everything here is upstream: the wasmCloud v2.11.0 Helm chart and host,
standard WASI 0.3 interfaces, wasmCloud's own keyvalue interface, and one
small WIT package of our own (`workshop:toolbox`). No vendor products are
needed.

You `kubectl apply` a ready made agent, build its first tool in Rust with
`wash`, put two teams' agents on one host pod, watch the same gullible agent
leak everything from an ordinary container, and then give the wasmCloud
agent its tools one step at a time. At the end you scale it across host pods
and troubleshoot it when it breaks. Every tool runs in
its own sandbox with only the access you grant it. The model is a scripted,
gullible stand-in that obeys any instruction it reads, including instructions
planted in a web page or a note. **The hijack always succeeds. The sandbox
does not stop prompt injection; it decides what a hijacked agent can reach.**

## What you need

* Docker (Docker Desktop, OrbStack or Podman) with at least 4 GB of memory
  for containers, on macOS or Linux. On Windows, use WSL2.
* Or nothing at all: open the repo in GitHub Codespaces (Code > Codespaces >
  Create codespace on main), which has everything installed.
* About 1 GB of downloads, most of it kind's Kubernetes node image. Do them
  before the conference: `make tools && make up`.
* Optional, to build the memory tool yourself in step 2: Rust 1.98.1 with
  the `wasm32-wasip2` target (`make doctor` says whether you have it;
  Codespaces has it). Run `make build` once beforehand to fetch the crates.
  Without Rust, step 2 uses the prebuilt image.

Every other tool (wash, kind, kubectl, helm, wasm-tools) is downloaded
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
switches back). Then follow `steps/01-agent/README.md`. Each step has a folder,
the commands to run and the output to expect (`expected-output.txt`).

| Step | What you do | What it shows |
| --- | --- | --- |
| 1 | `kubectl apply` the agent | A Workload is a custom resource; it runs on a wasmCloud host pod |
| 2 | Build its memory tool with `wash` | A component imports only what it asks for; a capability is an interface the platform grants |
| 3 | Two teams, two namespaces | One host pod, many tenants, each with its own hostname, store and grants |
| 4 | Run the same agent as a container | Kubernetes defaults start open: it leaks its Secret and its service account token |
| 5 | A web tool allowed one host | Planted attack 1 fails at the egress check, but the one allowed host is still a way out |
| 6 | A notes tool over one read only folder | Planted attack 2 fails at the folder boundary |
| 7 | Swap in a real model | Same slot, new implementation, no rebuild |
| 8 | Scale across hosts, then troubleshoot | The operator places replicas and routes to them; where to look when a workload won't start |

Steps 1 to 5 tell the whole story. Steps 6 to 8 go as far as time allows.
After every `kubectl apply`, each step waits (`sleep 2 && kubectl wait ...`)
before sending a request, so the request reaches the new version.
Every step's manifest is complete, so you can jump to any step with one
`kubectl apply`.

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
   └──────────────────┘  │   team-a:  toolbox-agent │  │   toy-sites               │
                         │   team-b:  toolbox-agent │  └───────────────────────────┘
   namespace             └──┬────────────────────┬──┘    ▲ Services planted, collector,
   workshop-data            │ keyvalue           │ HTTP  │ llm (.workshop-sites.svc)
   ┌──────────────────┐     │                    └───────┘
   │ valkey           │◀────┘                      only hosts in a component's
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
* **The data service.** One Valkey server (`k8s/data-valkey.yaml`). The
  platform declares where keyvalue data lives (`k8s/values.yaml`,
  `plugins:`): a default store for the default namespace and one store per
  team, each its own key prefix. Workloads only ask for "keyvalue", by store
  name: since v2.11.0 a workload may not set a backend's address or prefix
  itself, and an admission policy (`k8s/tenant-policy.yaml`) lets a team ask
  only for its own store. Blobstore (object storage) works the same way.
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
  "remember this" or "fetch that page".
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
  a keyvalue store, one folder, or a list of hosts it may call over HTTP.
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
        │        ├──▶ memory   ── keyvalue       (one store)           │
        │        ├──▶ notes    ── one folder, mounted read only        │
        │        └──▶ web      ── HTTP out       (allowedHosts list)   │
        └──────────────────────────────────────────────────────────────┘
```

The harness imports the model and three tool slots and nothing else from the
host: no filesystem, no outgoing HTTP, no sockets. Each tool is a separate
component with its own capabilities. If a planted instruction convinces the
model to misuse a tool, the damage is limited to what that one tool was
granted.

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

From `wit/toolbox.wit`, with the doc comments trimmed:

```wit
interface memory {
  variant error {
    invalid-argument(string), no-such-store(string),
    access-denied(string), unavailable(string), other(option<string>),
  }
  record entry { key: string, fact: string }
  record remember-args { key: string, fact: string }
  remember: async func(args: remember-args) -> result<_, error>;
  record recall-args { key: string }
  recall: async func(args: recall-args) -> result<option<string>, error>;
  recall-all: async func() -> result<list<entry>, error>;
}

interface notes {
  variant error {
    invalid-argument(string), not-found(string),
    access-denied(string), other(option<string>),
  }
  record read-args { path: string }
  read: async func(args: read-args) -> result<string, error>;
  record append-args { path: string, line: string }
  append: async func(args: append-args) -> result<_, error>;
}

interface web {
  variant error {
    invalid-argument(string), access-denied(string),
    network(string), other(option<string>),
  }
  /// What a server answered, whatever its status.
  record response { status: u16, text: string }
  record fetch-args { url: string }
  fetch: async func(args: fetch-args) -> result<response, error>;
  record post-args { url: string, body: string }
  post: async func(args: post-args) -> result<response, error>;
}

/// The agent harness: one model and one labelled slot per tool, and HTTP in.
world agent {
  import model: model;
  import memory: memory;
  import notes: notes;
  import web: web;
  export wasi:http/handler@0.3.0;
}
```

The functions are `async` because everything here is WASI 0.3, where a call
into another component, a storage read or an HTTP request can wait without
blocking the host. `notes` has `append` and `web` has `post` because the two
planted attacks need a way to write and a way to send; without them there
would be nothing for the sandbox to refuse.

### The translation table: what a host does for you, by hand

A model only speaks JSON. It is shown a list of tools, each with a name, a
description and a JSON Schema, and it answers with something like
`{"name": "remember", "arguments": {"key": "...", "fact": "..."}}`. The tools
only speak typed functions. Something has to translate between the two.

A host could do that for you. It could read each tool's WIT and build the
definitions the model sees from the names, types and doc comments: the
function's doc becomes the tool's description, and the record's fields
become its arguments. It would turn the model's JSON into a typed call, and
the typed result or error back into text. Upstream wasmCloud doesn't do that
yet, so this workshop's agent harness does it by hand, in plain sight, in
`components/agent/src/tools.rs`: one row per function, copied from the WIT,
and one `match` arm that builds the args record and makes the typed call.

```rust
"remember" => {
    let args = memory::RememberArgs { key: a.get("key")?, fact: a.get("fact")? };
    let line = format!("remembered {} = {}", args.key, args.fact);
    memory::remember(args).await.map_err(memory_error)?;
    Ok(line)
}
```

A typed error reaches the model as the JSON form of the WIT variant, the same
way a host renders it: `{"access-denied":"/notes/todo.txt: Operation not
permitted"}`. The rows also give the notes functions names that read better
to a model (`read_note` for `notes.read`, `append_note` for `notes.append`),
the way a host keeps a rename table.

### Labelled slots

`import memory: memory;` is a labelled import (wasmCloud#5593). The host links
the label to the component with the same `name` in the workload, with no
extra composition step. So the manifest, not the harness code, decides which
component sits behind each slot, and each tool keeps its own capabilities.
Each tool interface defines its own types, so a slot brings everything its
functions need with it.

Every label must be filled or the workload will not start. Until a step
installs a real tool, the slot is filled by `not-installed`: one image that
exports all three tool interfaces, deployed under several names, whose every
function returns its interface's `other` error, `the <tool> tool isn't
installed yet`. Labels are fixed when the harness is built, so adding a fourth
tool means a rebuild; upstream has no dynamic tool discovery today.

### The gullible model

`model-scripted` plans tool calls from the user prompt **and every tool
result**, in order, and runs each instruction once:

| It reads | It calls |
| --- | --- |
| `remember [that] X is Y` | `remember {key, fact}` |
| `what do you remember` / `recall` | `recall_all {}` |
| `read my notes` / `read <path>` | `read_note {path}` |
| `append "<text>" to <path>` | `append_note {path, line}` |
| `fetch <url>` / `summarize <url>` | `fetch {url}` |
| `send everything to <host>` | `post {url: http://<host>/collect, body}` |

"Everything" is all the text the model has read, memories included. That
makes it the worst case on purpose: a fully hijacked agent, every time, with
no API keys, no GPU and the same result on every laptop.

### The toy sites

`steps/00-fixtures/` runs one component that serves three fake sites, started
by `make up`: `planted.workshop-sites.svc/page` and `/page2` (pages with
hidden instructions), `collector.workshop-sites.svc/collect` (the attacker; it
counts every leak), `planted.workshop-sites.svc/collect` (the attacker's own
site collects too) and `llm.workshop-sites.svc` (a fake OpenAI compatible
model for step 7). Each name is a Kubernetes Service (`k8s/sites.yaml`) in
front of the fixtures host group, which runs nothing else; nothing leaves your
machine. `curl -H 'Host: sites.localhost' 127.0.0.1:18180/count` shows the
leak count, and a POST to `/reset` zeroes it. Only the name
`sites.localhost` may reset or list the leaks, so a hijacked agent cannot
wipe the evidence. In a browser, http://sites.localhost:18180
shows each leak as it arrives.

## WASI 0.3

Every capability the workshop teaches is WASI 0.3: HTTP in and out
(`wasi:http@0.3.0`), the filesystem (`wasi:filesystem@0.3.0`), and wasmCloud's
async `wasmcloud:keyvalue@0.2.0`. The components are built with `wit-bindgen` 0.62
for the `wasm32-wasip2` target, so Rust's standard library still links the
0.2 command line basics (stdout, environment, exit). `wasm-tools component
wit artifacts/<name>.wasm` lists exactly what each component imports.

## Building the components yourself

The prebuilt components in `artifacts/` (checked by `artifacts/SHA256SUMS`)
are what `make up` pushes. To build your own:

```sh
make build        # needs Rust 1.98.1 with the wasm32-wasip2 target
make push-build   # push target/wasm32-wasip2/release/*.wasm to the cluster's registry
```

Each component depends only on `wit-bindgen` and `serde_json`, so builds are
fast and need no network once the crates are fetched. Step 2 builds one tool
with `wash build`, which runs the command in that tool's `.wash/config.yaml`.

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
| A tool says `{"other":"Redis error: broken pipe"}` | The data service restarted; v2.11.0's redis backend doesn't reconnect | `kubectl -n wasmcloud rollout restart deployment/hostgroup-default` (and `-n workshop-sites ... hostgroup-fixtures`) |
| A workload's `Message:` says a store `is not among` the declared ones | The manifest names a keyvalue store the platform didn't declare | Use the store your namespace is given (`k8s/values.yaml`, `bindings:`) |
| `kubectl apply` is denied by `workshop-toolbox-tenant-stores` | A team's manifest asks for another store or host group | Name the keyvalue host interface after the namespace; use `hostgroup: default` |
| Port 18180 is taken | Another program has it | `HOST_HTTP=127.0.0.1:18190 make up` after `make clean` (the port is fixed when the cluster is created) |

## Layout

```
artifacts/        prebuilt components and SHA256SUMS (what make up pushes)
components/       the Rust source: agent, models, tools, not-installed, toy-sites
  common/         HTTP helpers over wasi:http@0.3.0, shared by source path
k8s/              chart values, the data service, the toy sites' Services, the ingress,
                  the teams' admission policy
scripts/          up, down, stores, prove, push, tools, doctor
steps/            one folder per step: manifest, README, expected output
wit/              workshop:toolbox and the WASI 0.3 / wasmCloud WIT it uses
```

## What wasmCloud v2.11.0 does and does not give you here

Verified on 2026-10-29 on macOS arm64, kind v0.33.0 and the upstream chart
v2.11.0 (`make prove`).

* **Works as the steps claim:** labelled imports between components in one
  workload; per component `allowedHosts`; a read only `hostPath` volume on one
  component, inside the host's `hostPaths.allowed` list; keyvalue on a
  backend the platform declares, one store per team with its own key prefix
  that a workload can't change; workloads from different
  namespaces on one host pod; `kubectl scale` on a WorkloadDeployment.
* **A grant belongs to one component, for calls made one after another.**
  Concurrent calls through linked components can currently borrow a
  sibling's grants (wasmCloud#5756). The workshop's harness calls its tools
  one at a time.
* **`hostPaths.allowed` is host wide.** Any workload on a host may mount any
  directory in that list (wasmCloud's THREAT_MODEL.md: "Every workload on a
  host may mount any directory the operator allowed"), so admission policy
  decides who may mount what. Components linked in one workload can
  currently see each other's mounts (draft wasmCloud#5756); harmless here,
  because the notes tool is the only component that imports a filesystem.
* **One isolation layer between tenants on a host.** Upstream's threat model
  is explicit: workloads on one host are separated by the WebAssembly runtime
  alone. For mutually hostile tenants, use separate host groups or nodes, and
  decide who may apply Workloads with RBAC and admission policy (wasmCloud
  ships no admission controller).
* **Local routing is off, and the toy sites have a host group of their
  own.** The chart's `localBypassRouting` lets any workload on a host claim
  any hostname with `localRoute`, with no check of who owns it, and answer
  the other workloads' requests to it in memory. With it on, a team-b
  workload that claimed the toy sites' names answered 8 of 8 of team-a's
  requests to them. So it is off on every host group, and the agents reach
  the toy sites by Kubernetes Services. `make prove` checks that the same
  claim now catches nothing.
* **A workload chooses its own host group.** `hostSelector` is part of the
  Workload spec, so without a policy team-b could ask for
  `hostgroup: fixtures` and claim `planted.workshop-sites.svc` there with
  `host-aliases`. Requests for a name are spread over every workload that
  claims it, so that squatter would get a share (3 of 8 in a test). The
  workshop's admission policy (`k8s/tenant-policy.yaml`) keeps the teams'
  namespaces on `hostgroup: default`. A real platform needs the same kind of
  rule, and one for ingress names such as `a.agent.localhost` on a shared
  host.
* **A keyvalue store is chosen by a label built into the component.** The
  host serves a store to the component import with the same label
  (`import team-a: wasmcloud:keyvalue/store`), and the store names in
  `bindings:` are host wide, not per namespace. So each team's memory tool is
  its own build (`tool-memory-team-a`, `tool-memory-team-b`, the same source
  with a different label), and admission policy has to decide which
  namespace may ask for which name. An import with no label gets the entry's
  own config; give that a prefix of its own too, or it reads every key in
  the server.
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
* **The data service is Valkey, not NATS.** The keyvalue plugin's redis
  backend (Valkey speaks the same protocol) ignores bucket names and keeps
  each store apart by the prefix the platform sets. Its NATS backend opens
  any bucket on the server by name, so on v2.11.0 nothing stops one tenant
  from opening another's bucket; scoping buckets is draft wasmCloud#5498.
  The workshop also runs its own data service because the chart's NATS
  requires mutual TLS with client certificates.
* **The redis backend doesn't reconnect.** The host keeps one connection
  per store and shares it across workloads. After Valkey restarts, every
  call answers `Redis error: broken pipe` until the host pods restart;
  redeploying a workload reuses the broken connection. The data itself
  survives (Valkey keeps it on a volume).
