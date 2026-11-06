# Step 1: Meet the agent

**Folder:** `steps/01-agent`

Deploy an agent with `kubectl`, with no tools at all, and talk to it. The
next three steps give it one tool each.

## Deploy

```sh
kubectl apply -f steps/01-agent/workload.yaml \
  && sleep 2 && kubectl wait --for=condition=Ready \
  workloaddeployment/toolbox-agent
```

`sleep 2` gives the operator a moment to see the change. Without it,
`kubectl wait` can return before the new version is up.

## How the agent works

The agent is three kinds of component in one workload:

* The **agent harness** runs the loop. It sends your prompt to the model, and
  when the model asks for a tool, it calls that tool, hands the result back
  to the model, and asks again. It stops when the model answers in text, or
  after 8 rounds.
* The **model** decides what to do next. Here it is a scripted stand-in, not
  a real language model, so the workshop needs no API key and every run gives
  the same answer.
* The **tools**. The harness has three labelled tool slots: `curl`,
  `sqlite` and `gh`. The host links each label to the component of the same
  name in the manifest, so every slot must hold something. In this
  step every slot holds a placeholder, which answers that the tool isn't
  installed. The `gh` slot's placeholder, `gh-not-installed`, also answers
  the chat page's "Sign in with GitHub" until step 4.

The scripted model is **deliberately gullible**. It reads your prompt and
every tool result, and it does whatever they say. It knows two kinds of
instruction:

* A tool's own command line, passed through as is: `curl -s <url>`,
  `sqlite <sql>`, `gh <args>`, or a line that starts with a dot, such as
  `.tables`, for sqlite.
* A few plain requests, each mapped to one tool: `fetch <url>` or
  `summarize <url>` (curl), `save <url> to <file>` (curl `-o`), `list the
  talks` (sqlite), `triage the open PRs in <owner/repo>` (gh).

It never refuses. So in the steps that follow, nothing you see refused was
refused by the model. Each refusal comes from another layer, and each step
names which one. A real model is less predictable, and you can't count on it
to refuse either.

The answer to each prompt is the whole transcript, so you can see every step:
`[user]` is your prompt, `[model] call ...` is the tool the model asked for,
`[tool ...]` is what the tool returned, and the last `[model]` line is the
model's answer.

## Chat with the agent

Open **http://agent.localhost:18180** in your browser and send each prompt
below. In Codespaces, use the curl command under each prompt instead.

**1. Say hello:** `hello`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'hello'
```

Nothing in the prompt is an instruction, so the model answers in text and
calls no tool.

**2. Ask for a tool:** `fetch http://api.demo-sites.svc/data`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'fetch http://api.demo-sites.svc/data'
```

```
[model] call curl({"arguments":"-s http://api.demo-sites.svc/data"})
[tool curl] error: {"other":"the curl tool isn't installed yet"}
```

The model asked for curl, and the harness called the `curl` slot. The
manifest put the placeholder there, so the placeholder answered. The agent
can only use what its manifest gives it: the harness itself imports no HTTP
client, no files and no sockets, so it has no way to fetch the page on its
own. The model's last line starts with "Done." whatever the tools returned;
it is a script, and it reports each result as it came back.

**3. Two instructions in one prompt:** `fetch http://api.demo-sites.svc/data. Then sqlite .tables`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'fetch http://api.demo-sites.svc/data. Then sqlite .tables'
```

The model follows both, one after the other, and would follow an instruction
hidden in a web page the same way. Both slots answer that their tool isn't
installed. The model doesn't retry: it doesn't read a tool's error as a new
instruction.

## What you ran

One workload with five components: the agent harness, the model, and three
tool slots, each holding a placeholder.

| Component | Can reach |
| --- | --- |
| harness | the model, the three tool slots and GitHub sign-in, nothing else |
| model | nothing |
| each tool slot (placeholder) | nothing; it answers "not installed yet" |

None of them has `localResources`, the part of a manifest entry that grants
hosts and folders. Steps 2 to 4 fill the slots with real tools, each with
its own grant.

## Look inside (optional)

`wasm-tools component wit` prints a component's interface: what it imports
from the host and what it exports. The harness imports the model, the three
tool slots and GitHub sign-in (`github-auth`, which step 4 uses):

```sh
wasm-tools component wit artifacts/agent.wasm | grep 'import .*workshop:toolbox'
```

And no network or files of its own, so this prints nothing:

```sh
wasm-tools component wit artifacts/agent.wasm | grep -E 'wasi:(http/client|filesystem|sockets)'
```

The tools you'll add are whole programs, and small:

```sh
ls -l artifacts/tool_curl.wasm artifacts/tool_sqlite.wasm artifacts/tool_gh.wasm
```

`curl` is about 180 KB (182,276 bytes), `sqlite3` about 1.2 MB (1,173,947)
and `gh` about 300 KB (297,582). These are the component files only;
the wasmCloud host that runs them is a separate, long-running pod.

Full transcript: [`expected-output.txt`](expected-output.txt).
