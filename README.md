# Sandboxing on Kubernetes with WebAssembly
## Agent harnesses and tools on wasmCloud

In this hands-on workshop for KubeCon + CloudNativeCon NA 2026, you will: 

* Deploy a sandboxed agent to your own cluster with **wasmCloud**, a CNCF project 
* Give the agent `curl` and `sqlite3` tools, then try each tool's most famous escapes and watch them
fail 
* Give your agent a real job, triaging pull requests on GitHub (optionally signed
in with your own account)

On your own machine, after `make up`, you can read these steps in your browser at
http://guide.localhost:28180, with a Copy button on every command. 

In Codespaces, follow the raw README files in each `/steps/` directory.

## Watch it first (optional)

[`docs/walkthrough.cast`](docs/walkthrough.cast) is a recording of the whole
workshop, run from a fresh clone with exactly the commands in these READMEs
and narrated in their words (about 18 minutes, step 4 signed out). It plays
with asciinema, which `make tools` does not install: install it by following
[asciinema's getting started guide](https://docs.asciinema.org/getting-started/#__tabbed_1_4).
Then play it in a terminal (space pauses):

```sh
asciinema play docs/walkthrough.cast
```

## Requirements

* Docker (Docker Desktop, OrbStack, or Podman) with 4 GB or more of memory for
  containers, on macOS or Linux. On Windows, use WSL2.
* Or nothing at all: GitHub Codespaces (see below).
* About 1 GB for toolchains and dependencies, installed with `make tools && make up`.
* For step 4, the internet and a GitHub account. Sign in to github.com in
  your browser before you start. Signing the agent in to GitHub is optional,
  but on a shared network it keeps step 4 working: see "Running the workshop
  for a group" below.
* Optional, to build `curl` yourself in step 2: Rust 1.98.1 with the
  `wasm32-wasip2` target, and one `make build` beforehand to fetch the
  crates. Without it, step 2 uses the prebuilt image.

`make tools` downloads every other tool (wash, kind, kubectl, helm,
wasm-tools) into `.tools/`, pinned and checksum checked. Nothing is installed
outside the repo and nothing needs sudo.

## Set up

Clone the repo and go into it:

```sh
git clone https://github.com/cosmonic-labs/sandbox-workshop-kcna26
```

```sh
cd sandbox-workshop-kcna26
```

Download the pinned tools into `.tools/`:

```sh
make tools
```

Use those tools in this terminal:

```sh
export PATH="$PWD/.tools:$PATH"
```

Check this machine (changes nothing):

```sh
make doctor
```

Start the kind cluster and the wasmCloud chart (about 2 minutes the first
time):

```sh
make up
```

Every command in the steps runs from the `sandbox-workshop-kcna26` folder. In
each new terminal, `cd` there and run the `export PATH` line again, so
`kubectl` and `wasm-tools` are the pinned ones. `make up` creates a kind
cluster named `workshop-toolbox` and switches your kubectl context to it.

## Steps

| Step | Folder | What you'll see |
| --- | --- | --- |
| 1 | `steps/01-agent` | The agent with no tools: how it and its scripted model work, and that it can use only what its manifest gives it |
| 2 | `steps/02-curl` | `curl` (prebuilt, or build your own) gets one host; its escapes (metadata service, other hosts, redirects, files) are refused |
| 3 | `steps/03-sqlite` | `sqlite3`'s escapes (`.shell`, `ATTACH`, extensions) go nowhere; then the same talks behind a REST service and an MCP server, one grant per hop |
| 4 | `steps/04-github` | A real job: triage wasmCloud's pull requests with `gh`, signed in or not; the GitHub capability refuses writes, and the token never leaves the platform |

Each step includes the commands and the output to expect
(`expected-output.txt`).

**Fell behind?** Every step's manifest is complete, so you can jump to any
step with its one `kubectl apply`.

## Chat with the agent

Step 1 deploys the agent, and its chat page comes up then: after step 1's
Deploy command prints `condition met`, open **http://agent.localhost:28180**
in your browser. Before that, the page doesn't load. On your own machine or
in a local dev container, browsers send `*.localhost` to your own machine,
and the dev container forwards port 28180, so it works in both.

<!-- codespaces -->
In Codespaces, run the `./ask` command each step gives instead, from the
`sandbox-workshop-kcna26` folder. `./ask 'hello'` sends the prompt to the
agent and prints its answer: the same request the chat page sends, to
`127.0.0.1:28180` with the header `Host: agent.localhost`. On a cluster
made with another `HOST_HTTP`, set it for `./ask` too, for example
`HOST_HTTP=127.0.0.1:28190 ./ask 'hello'`.
<!-- /codespaces -->

## GitHub Codespaces

No Docker? Open the repo in Codespaces (Code > Codespaces > Create codespace
on main) on a 4 core machine. Everything is installed, including Docker. Run
`make up` in its terminal and follow the steps with `./ask`. A browser pointed at
the forwarded port shows nothing useful, because it sends the codespace's own
host name, not `agent.localhost`.

## Troubleshooting

| You see | Fix |
| --- | --- |
| `make doctor`: Docker is not running, or has under 4 GB | Start Docker, or raise its memory limit (Docker Desktop: Settings > Resources) |
| `make doctor`: Rust is not on your PATH | Only needed to build; run the `fix:` line it prints |
| `curl: (7) Failed to connect to 127.0.0.1 port 28180` | The cluster is not up: `make up` |
| The chat page at http://agent.localhost:28180 doesn't load, or shows 404 or 502 Bad Gateway; or `./ask` says `no answer from the agent` | No agent is deployed yet: run step 1's Deploy command and wait for `condition met`. Right after a later step's Deploy, wait for its `condition met` and try again |
| Safari can't find `guide.localhost` or `agent.localhost` (older macOS) | Open the same address in Chrome or Firefox, which always send `*.localhost` to your own machine |
| `kubectl` talks to another cluster | `kubectl config use-context kind-workshop-toolbox` |
| `kubectl wait` times out | The host is still pulling: `kubectl get workloads -A`, then try again |
| Port 28180 is taken | `make clean`, then `HOST_HTTP=127.0.0.1:28190 make up` (the port is fixed when the cluster is created) |
| Step 4: `GitHub's rate limit for unauthenticated core requests is used up` | Click **Sign in with GitHub** on the chat page, or follow step 4's samples; prompts 6 to 8 need no GitHub |

`make down` changes nothing. `make clean` deletes the cluster and its
registry, and switches kubectl back to the context you had before `make up`.

## Running the workshop for a group

Step 4 reads live data from GitHub. Signed out, GitHub allows 60 requests an
hour for each public IP address, and everyone on the same Wi-Fi shares one.
The step uses about 10 requests per person, so a shared network runs out
after about six people. Signed in, each person gets their own 5,000 an hour.

If you're teaching this workshop to a group:

* Ask everyone to bring a GitHub account.
* At step 4, after its `kubectl apply`, have everyone click **Sign in with
  GitHub** on the chat page. Each step deploys a new workload, which starts
  signed out, so signing in earlier doesn't carry over.
* In the room, use the browser sign-in. The terminal sign-in
  (`make github-sign-in`) uses GitHub's device flow, which allows 50 sign-ins
  an hour for the whole app. Keep it for Codespaces, which reaches GitHub from
  its own address.
* Sign-in asks for no scopes, so the token can read only what anyone can
  read. It stays in the github-api plugin's memory, expires after 8 hours,
  and can be revoked under GitHub > Settings > Applications.
* Anyone can follow along without signing in: prompts 6 to 8 send nothing to
  GitHub, and step 4 shows a sample of every answer.

## How it fits together

```
  curl or browser ──▶ 127.0.0.1:28180 ──▶ ingress (picks a Service by Host)
                                             │
            ┌────────────────────────────────┴───────────────┐
            ▼                                                ▼
  host pod, group "default"                       host pod, group "mock-sites"
    workload toolbox-agent                          mock sites: api, attacker; the guide
                                                    talks-api (its own folder)
                                                    talks-mcp ──▶ talks.mock-sites.svc only
      harness ──▶ model
        ├──▶ sign-in  github-api plugin ──▶ github.com (OAuth, no scopes)
        └──▶ tools    toolbox plugin, which calls the tools in this workload:
               ├──▶ curl     allowedHosts only ──▶ api.mock-sites.svc, /data for -o
               ├──▶ sqlite   /data, read only
               ├──▶ mcp      allowedHosts only ──▶ mcp.mock-sites.svc (the MCP server)
               └──▶ gh       github-api plugin ──▶ api.github.com (GET, allowlisted paths)
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
  only what its entry in the manifest grants. It finds its tools through
  `tools`, an interface from Cosmonic's agent SDK (`cosmonic:agent`), which
  the platform's toolbox plugin serves by listing and calling the tool
  components in the agent's own workload.

The mock sites ([guide.localhost:28180/mock-sites](http://guide.localhost:28180/mock-sites/)) are fake websites inside the
cluster, so steps 1 to 3 need no internet. One is an ordinary API, the host
`curl` is allowed to reach. The other, `attacker.mock-sites.svc`, plays the
server of someone who has hijacked your agent: the place an instruction
planted in a web page or a database would tell the agent to send what it can
read. It is a real server on the cluster network and it answers anything sent
to it, so when a request to it is refused, the agent's grants are the only
thing that stopped the leak. Step 3 adds two more on the same host group: a
REST service over the talks and an MCP server in front of it, each an
ordinary HTTP component with its own grant.

## Notes and caveats

* The github-api plugin keeps each sign-in in its memory, keyed by the
  workload's id, and nothing clears the entry when a workload goes away: a
  replaced workload's token stays in the plugin's memory, unused, until the
  host restarts. A real platform would keep tokens in a secrets store and
  drop them when the workload is deleted.
* GitHub sign-in's browser flow comes back to a fixed address, the
  plugin's `redirect-uri` in `k8s/values.yaml`:
  `http://agent.localhost:28180/github/callback`. A cluster made on another
  port (`HOST_HTTP`) can't receive it; use `make github-sign-in` there. The
  authorization code passes through the agent harness on its way to the
  plugin: it works once and only with the plugin's PKCE verifier and client
  secret, and the harness never sees a token. 
* The GitHub client secret is committed on purpose, as GitHub's CLI commits its own
  (`cli/cli`, `internal/authflow/flow.go`). It identifies the app and gets
  no token without a person approving, and PKCE binds each code to the
  plugin that asked for it. 
* The chart's NetworkPolicies are off in `k8s/values.yaml` because kindnet on
  kind v0.33.0 dropped their DNS replies. Keep them on in a real cluster.
* `artifacts/` holds the prebuilt components `make up` pushes, so the
  workshop needs no Rust. They are built from this source in a pinned
  linux/arm64 build image (`build.Dockerfile`), which makes them
  reproducible: `make check-artifacts` rebuilds them and fails if a file, or
  a plugin digest pinned in `k8s/values.yaml`, differs. CI runs it on every
  push and pull request, and publishes each build from `main` to
  `ghcr.io/cosmonic-labs/sandbox-workshop-kcna26/<name>`, with the same
  digests `k8s/values.yaml` pins. After changing a component, run
  `make artifacts` (needs Docker) and commit the result.
* CI (`.github/workflows/ci.yml`) also runs the components' unit tests and
  clippy, and runs the workshop itself on every push and pull request:
  `make up` on a fresh kind cluster, then `make verify`.

## License

Apache-2.0. See [LICENSE](LICENSE).
