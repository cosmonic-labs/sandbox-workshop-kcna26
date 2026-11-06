# Step 4: Triage pull requests on GitHub

**Folder:** `steps/04-github`

The first three steps tried each tool's escapes. This one gives the agent a
real job: triage the open pull requests in
[wasmCloud/wasmCloud](https://github.com/wasmCloud/wasmCloud). It uses a
`gh` tool that reads GitHub the way GitHub's own `gh` command does, and you
can sign it in to GitHub with your own account, scoped to this demo.

The `gh` component holds no token and imports no HTTP client, no sockets and
no files. Its only way to GitHub is `workshop:toolbox/github-api`, served by
the **github-api plugin**: a host component plugin, which is a component the
platform installs on the host group rather than a workload. The plugin builds
every request itself. It sends only `GET`, only to `api.github.com`, and only
for the paths `gh` needs: pull requests, their files, reviews, comments and
checks, and the issue search. It refuses everything else and says so.

The same plugin runs the sign-in. The agent harness imports its other
interface, `github-auth`, for the chat page's **Sign in with GitHub**. The
plugin does the whole sign-in itself and keeps the token GitHub hands it, so
the token never leaves the plugin. The harness and the tool never see it.

## Three layers

| Layer | What it decides here |
| --- | --- |
| wasmCloud grant (the manifest and the host group) | Which components may call the plugin: only workloads that declare it under `hostInterfaces`, and in this workload only `gh` imports `github-api` and only the harness imports `github-auth`. Where the plugin may connect: its `allowedHosts` are `github.com` (sign-in) and `api.github.com`. Every other tool keeps its own grant, so `curl` can't reach GitHub. |
| The github-api plugin's own rules | `GET` only, the path allowlist, and whose token goes on a request: the host tells the plugin which workload is calling (`wasmcloud:host/identity`), and each workload gets only its own sign-in. A workload the host can't name gets nothing. |
| GitHub | What the token may do. It is an OAuth token with no scopes: it reads public data, like no token at all, with a signed-in user's higher rate limit. |

## Deploy

```sh
kubectl apply -f steps/04-github/workload.yaml \
  && sleep 2 && kubectl wait --for=condition=Ready \
  workloaddeployment/toolbox-agent
```

The same agent as step 3, with the real tool in its gh slot and one new
entry under `hostInterfaces`:

```yaml
- namespace: workshop
  package: toolbox
  interfaces: [github-api, github-auth]
```

## Try each command

This step needs the internet: the answers come live from GitHub, so yours
will differ from the samples here. Type each prompt into the chat at
http://agent.localhost:18180, or run the curl command under it.

**1. Sign in (optional):** click **Sign in with GitHub** at the top of the
chat page.

Your browser goes to github.com, which asks you to approve the "KubeCon
sandbox workshop" app (sign in to GitHub first if you aren't). Approve, and
GitHub sends you back to the chat page, which now says
`Signed in to GitHub as <you>`. The app asks for no scopes, so the token can
read only what anyone can read.

Why sign in at all: without a token, GitHub allows 60 requests an hour,
shared by everyone behind the same IP address, which in a conference room is
everyone on the Wi-Fi. Signed in, you get your own 5,000. Each prompt below
costs 1 to 3 requests, and every `gh` answer ends with how many are left. If
you'd rather not sign in, or the limit runs out, follow along with the
samples here; the refusals in prompts 6 to 8 send nothing to GitHub.

How it works. This is GitHub's OAuth web application flow, with PKCE. The
plugin makes a random `state` and a secret verifier, and sends your browser
to GitHub with the state and a hash of the verifier. GitHub sends your
browser back to `http://agent.localhost:18180/github/callback` with the same
state and a code. The harness passes both to the plugin, which checks the
state and trades the code for a token, proving it holds the verifier and
the OAuth app's client secret. So the agent does see one thing: that code.
It works once, for a few minutes, and only together with the verifier and
the secret, which never leave the plugin. The token itself never reaches the
agent.

**In Codespaces, or without a browser on the same machine,** the browser
can't come back to `agent.localhost`, so sign in from the terminal with
GitHub's device flow instead:

```sh
make github-sign-in
```

```
Open https://github.com/login/device and enter WDJB-MJHT
Waiting for GitHub (Ctrl-C to stop waiting; the code expires in 14 minutes)...
Signed in to GitHub as <you>
```

Open the link in any browser, on any device, enter the code and approve.
`make github-sign-out` signs out.

The same with curl. Start sign-in, which prints the code and the link:

```sh
curl -s -X POST -H 'Host: agent.localhost' http://127.0.0.1:18180/github/sign-in
```

Enter the code, then check; this prints `{"login":"<you>","state":"signed-in"}`:

```sh
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/github/status
```

Sign out:

```sh
curl -s -X POST -H 'Host: agent.localhost' http://127.0.0.1:18180/github/sign-out
```

Use the browser when you can. GitHub lets one OAuth app start only 50 device
flow sign-ins an hour, for everyone using it, and a full room would run
through that. The browser flow has no such limit.

**2. The open pull requests** (allowed): `triage the open PRs in wasmCloud/wasmCloud`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'triage the open PRs in wasmCloud/wasmCloud'
```

The model turns this into `gh pr list -R wasmCloud/wasmCloud --limit 10`:

```
[tool gh] ok: 10 open pull requests in wasmCloud/wasmCloud, by updated (desc), page 1: #5886 test(operator): cover workload placement and lifecycle recovery (by ricochet, main <- ...
```

Each line is one pull request: number, title, author, branches, last
update, and labels and requested reviewers when there are any.

**3. One pull request** (allowed): `gh pr view 5885`

Pick any number from your list. The commands here use #5885, which is
merged, so it reads the same for everyone.

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'gh pr view 5885'
```

```
[tool gh] ok: #5885 feat(wash): allow configuring the host ID https://github.com/wasmCloud/wasmCloud/pull/5885 State: merged Author: ricochet | ...
```

State, author, branches, whether it can merge, its size, labels, who was
asked to review and what each reviewer decided, then the description. Add
`--comments` for the conversation and review comments.

**4. Its checks** (allowed): `gh pr checks 5885`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'gh pr checks 5885'
```

```
[tool gh] ok: Checks for #5885 in wasmCloud/wasmCloud at b156b0be: 72 passing, 0 failing, 0 pending ...
```

Failures come first, so a red pull request shows why at the top.

**5. Its diff** (allowed): `gh pr diff 5885`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'gh pr diff 5885'
```

```
[tool gh] ok: 2 files changed in wasmCloud/wasmCloud#5885 (page 1): modified crates/wash/src/cli/host.rs (+61 -1) ...
```

Each changed file, then each file's patch. curl's transcript cuts each tool
result to one line of 300 characters; the model reads all of it. The chat
page shows the whole result, and draws a diff as one: each file's header,
added lines in green with their `+`, removed lines in red with their `-`.

**6. curl straight at GitHub** (refused: the grant):
`curl -s https://api.github.com/repos/wasmCloud/wasmCloud/pulls`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'curl -s https://api.github.com/repos/wasmCloud/wasmCloud/pulls'
```

`access-denied: api.github.com: this tool may not reach that host`. Signed
in or not, curl's `allowedHosts` is the demo API host alone, and curl holds no
token anyway. Nor is there a token file for it to read: the plugin keeps the
token in its memory and writes it nowhere, and curl's only folder is `/data`.

**7. A write, through the capability** (refused: the plugin):
`gh api -X POST /repos/wasmCloud/wasmCloud/issues/5885/comments`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'gh api -X POST /repos/wasmCloud/wasmCloud/issues/5885/comments'
```

`gh api` sends exactly this request through the capability, past the tool's
own commands (`gh pr merge` and the other writes are refused by the tool's
own code first). The plugin refuses it before anything leaves the host:
`access-denied: github-api plugin: only GET requests are allowed`. That holds
when you are signed in, too: the plugin checks the method and the path
before it looks for a token. A read outside the allowlist is refused the
same way. `gh api /user`, which would say whose token it is, gets
`github-api plugin: /user is not on this plugin's path allowlist`.

**8. What the tool can touch** (the imports):

```sh
wasm-tools component wit artifacts/tool_gh.wasm \
  | grep -E '^  import (workshop|wasi:http|wasi:sockets|wasi:filesystem|wasmcloud)'
```

```
  import workshop:toolbox/github-api@0.4.0;
```

One import: the capability. No HTTP client, no sockets, no files, no secrets.
Whatever the model asks `gh` to do, the only door out is the plugin.

## What you ran

| Piece | Holds | Can reach |
| --- | --- | --- |
| `gh` tool | nothing | the github-api capability, nothing else |
| agent harness | nothing | github-auth (sign-in), the model and the tool slots |
| github-api plugin (platform) | each workload's token, in memory | `github.com` and `api.github.com` only (`allowedHosts`), `GET` only, the allowlisted paths only |

The platform side is in `k8s/values.yaml`: the plugin, its `allowedHosts`,
and the GitHub OAuth app sign-in uses (`client-id` and `client-secret`).

## What this doesn't do

* **The token is yours, with no scopes.** It reads what anyone can read on
  GitHub, with your rate limit. It lives in the plugin's memory, for this
  workload only: applying another manifest makes a new workload and signs you
  out, and a host restart signs everyone out. **Sign out** forgets it; GitHub
  keeps it valid until you revoke it under Settings > Applications >
  Authorized OAuth Apps.
* **The agent sees the browser flow's code.** GitHub sends it back through
  the harness. It is single use, expires in minutes, and is useless without
  the plugin's PKCE verifier and the app's client secret. The agent never
  sees a token.
* **The client secret is in the repo.** That is on purpose, as GitHub's own
  CLI does: it identifies the app, and gets no token without a person
  approving on github.com. It will be rotated after the event.
* **What the agent reads, it can pass on.** The plugin decides what reaches
  GitHub, not where the answers go next. The agent could hand what `gh` read
  to `curl`, which may still reach its one allowed host. Grants limit reach,
  not data flow.
* **The scripted model reads `gh`'s results as data.** Real pull request
  titles say things like "get rid of", which it would otherwise obey. Every
  other tool's output still counts as instructions.
* **One call at a time.** The harness awaits each tool call before the next,
  which keeps clear of wasmCloud#5756 (see the main README's known limits).
* **Sign-in needs an OAuth app.** Until the platform sets the plugin's
  `client-id` and `client-secret`, Sign in with GitHub says sign-in isn't set
  up, and everything else works signed out.

Full transcript: [`expected-output.txt`](expected-output.txt). GitHub's
answers are live, so the transcript writes each one as
`<live data from GitHub>`; `make verify` checks their shape and every
refusal.
