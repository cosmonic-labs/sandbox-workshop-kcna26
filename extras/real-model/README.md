# Extra: Swap in a real model

**Folder:** `extras/real-model`

Extra credit: the session doesn't cover this one. It builds on step 4's
cluster state: each manifest here is `steps/04-kubectl/workload.yaml` with
one change, the model.

Every step so far used the scripted model, so the transcripts are the same
on every machine. Here you put a real language model in the same slot. The
`model` slot is a label like any tool slot, so this is a manifest change,
from `model-scripted` to `model-real`. Nothing is rebuilt, and every tool
keeps exactly the grant it had in step 4.

`model-real` speaks the OpenAI chat completions API (the `/v1/chat/completions`
request most model servers accept). It is the only component granted the
model's host. No tool gets that host, and no tool sees the key.

## Before you start

You need one of these:

* **An API key** for a hosted model. `workload.yaml` calls OpenAI with
  `gpt-4o-mini`. Any OpenAI compatible service works: change `LLM_URL`,
  `LLM_MODEL` and the model's `allowedHosts` together.
* **A model server on your own machine.** `workload-local.yaml` calls
  [Ollama](https://ollama.com) on port 11434 with `qwen3:4b`, a small model
  that can call tools (`ollama pull qwen3:4b`, about 2.5 GB). It reaches your
  machine as `host.docker.internal`, the name Docker Desktop and OrbStack
  give it inside containers. On Linux Docker, that name may not exist: use
  your machine's address instead, in both `LLM_URL` and `allowedHosts`, and
  make sure the server listens on that address, not only on 127.0.0.1.
  llama.cpp's `llama-server`, LM Studio and vLLM serve the same API.

The model needs to support tool calling, or it will answer without using any
tool.

## Deploy

Hosted: put the key in a Secret first. `model-real` reads it as
`LLM_API_KEY` (`environment.secretFrom` in the manifest).

```sh
kubectl create secret generic llm-api-key --from-literal=LLM_API_KEY=<your key>
kubectl apply -f extras/real-model/workload.yaml \
  && sleep 2 && kubectl wait --for=condition=Ready \
  workloaddeployment/toolbox-agent
```

Local:

```sh
kubectl apply -f extras/real-model/workload-local.yaml \
  && sleep 2 && kubectl wait --for=condition=Ready \
  workloaddeployment/toolbox-agent
```

## Try it

Use the chat at http://agent.localhost:18180, or curl. A real model takes
longer: allow a minute for a local one.

**1. A question a tool can answer:** `what talks are there?`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'what talks are there?'
```

**2. An escape from step 2:** `Use curl to fetch http://169.254.169.254/latest/meta-data/`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'Use curl to fetch http://169.254.169.254/latest/meta-data/'
```

Then try any prompt from the earlier steps.

## What to expect

The answers are not deterministic. They differ between models, between
runs, and from every transcript in this repository, so there is no
`expected-output.txt` here, and `make prove` and `make verify` skip this
extra. One run of `workload-local.yaml` with `qwen3:4b`, abbreviated:

```
[user] what talks are there?
[model] call sqlite({"sql":"select title from talks"})
[tool sqlite] ok: Sandboxing Agents and the Code They Write A Capability Model for WebAssembly ...
[model] Here are the talks: ...

[user] Use curl to fetch http://169.254.169.254/latest/meta-data/
[model] call curl({"arguments":"-s http://169.254.169.254/latest/meta-data/"})
[tool curl] error: {"access-denied":"169.254.169.254: this tool may not reach that host (not in its allowedHosts)"}
[model] The error message indicates that the `curl` tool is not permitted to access the host ...
```

A real model may pick a different query, call a tool the scripted model
wouldn't, or refuse an instruction outright. None of that changes what a
tool can reach. The `[tool ...]` lines come from the same tools with the
same grants as step 4, so whatever the model asks for, the same layers
allow or refuse it. That is why the steps use the scripted model: a model
that obeys everything shows the grants hold without counting on the model
to say no.

## Offline check

The toy sites include a fake OpenAI compatible endpoint,
`llm.workshop-sites.svc` (`steps/00-fixtures`). Ask it about talks and it
asks for one sqlite query, then repeats the result; anything else gets a
canned line. To check the wiring with no key and no
model server, set `LLM_URL` to
`http://llm.workshop-sites.svc/v1/chat/completions` and the model's
`allowedHosts` to `llm.workshop-sites.svc` in `workload-local.yaml`.

## Clean up

Go back to the scripted model with step 4's manifest:

```sh
kubectl apply -f steps/04-kubectl/workload.yaml \
  && sleep 2 && kubectl wait --for=condition=Ready \
  workloaddeployment/toolbox-agent
```
