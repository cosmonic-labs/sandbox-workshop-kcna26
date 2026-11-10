# Step 1: Meet the agent

**Folder:** `steps/01-agent`

Deploy an agent with `kubectl` and talk to it. 

## Deploy

The agent's chat page doesn't exist until this command finishes. Run it and
wait:

```sh
kubectl apply -f steps/01-agent/workloaddeployment.yaml \
  && sleep 2 && kubectl wait --for=condition=Ready \
  workloaddeployment/toolbox-agent
```

It has finished when it prints `condition met`:

```text
workloaddeployment.runtime.wasmcloud.dev/toolbox-agent created
workloaddeployment.runtime.wasmcloud.dev/toolbox-agent condition met
```

**Now open the chat:** **http://agent.localhost:28180** in your browser.

<!-- codespaces -->
In Codespaces there is no chat page: you'll send each prompt with `./ask`
instead.
<!-- /codespaces -->

## How the agent works

* The **agent harness** runs the loop. It sends your prompt to the model, and
  when the model asks for a tool, it calls that tool, hands the result back
  to the model, and asks again. It stops when the model answers in text, or
  after 8 rounds.
* The **model** decides what to do next. Here it is a scripted stand-in, not
  a real language model, so every run gives the same answer. It can be swapped
  in for a real model.
* The **tools** are components in the same workload, each with its own
  grant. The harness finds them through `tools`, an interface from Cosmonic's
  agent SDK that the platform's toolbox plugin serves: it lists the tools this
  workload's manifest gives the agent, and calls the one the model asks for.
  In this step the manifest gives none.
  
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

Send each prompt below in the chat you opened after Deploy,
http://agent.localhost:28180.

<!-- codespaces -->
In Codespaces, run the `./ask` command under each prompt instead: it sends
the prompt to the agent and prints its answer.
<!-- /codespaces -->

**1. Say hello** 

In the chat:

```chat
hello
```

<!-- codespaces -->
In Codespaces:

```sh
./ask 'hello'
```
<!-- /codespaces -->

Nothing in the prompt is an instruction, so the model answers in text and
calls no tool.

**2. Ask for a tool** 

In the chat:

```chat
fetch http://api.mock-sites.svc/data
```

<!-- codespaces -->
In Codespaces:

```sh
./ask 'fetch http://api.mock-sites.svc/data'
```
<!-- /codespaces -->

```text
[model] call curl({"arguments":"-s http://api.mock-sites.svc/data"})
[tool curl] error: {"not-found":"no tool named curl in this workload"}
```

The model asked for curl anyway, and the toolbox plugin refused: this
workload has no curl. The agent can only use what its manifest gives it: the harness itself imports no HTTP
client, no files and no sockets, so it has no way to fetch the page on its
own. The model's last line starts with "Done." whatever the tools returned;
it is a script, and it reports each result as it came back.

**3. Two instructions in one prompt** 

In the chat:

```chat
fetch http://api.mock-sites.svc/data. Then sqlite .tables
```

<!-- codespaces -->
In Codespaces:

```sh
./ask 'fetch http://api.mock-sites.svc/data. Then sqlite .tables'
```
<!-- /codespaces -->

The model follows both, one after the other, and would follow an instruction
hidden in a web page the same way. Both calls are refused: the workload has
neither tool.

## What you ran

| Component | Can reach |
| --- | --- |
| harness | the model, its tools (none yet) and GitHub sign-in, nothing else |
| model | nothing |

None of them has `localResources`, the part of a manifest entry that grants
hosts and folders. 

In the next steps we'll give the agent some real tools, each with
their own grants.

## Look inside (optional)

`wasm-tools component wit` prints a component's interface: what it imports
from the host and what it exports. The harness imports the model, the agent
SDK's `tools` and GitHub sign-in (`github-auth`, which step 4 uses):

```sh
wasm-tools component wit artifacts/agent.wasm \
  | grep -e 'import model:' -e 'cosmonic:agent/tools' -e 'toolbox/github-auth'
```

And no network or files of its own, so this prints nothing:

```sh
wasm-tools component wit artifacts/agent.wasm | grep -E 'wasi:(http/client|filesystem|sockets)'
```

The tools you'll add are whole programs, and small:

```sh
ls -l artifacts/tool_curl.wasm artifacts/tool_sqlite.wasm artifacts/tool_gh.wasm
```

* `curl` is about 180 KB (183,702 bytes) 
* `sqlite3` is about 1.2 MB (1,178,728)
* `gh` is about 300 KB (296,735) 

These are the component files only; the wasmCloud host that runs them is a separate, long-running pod.

Full transcript: [`expected-output.txt`](expected-output.txt).
