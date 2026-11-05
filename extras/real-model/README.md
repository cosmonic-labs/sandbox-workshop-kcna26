# Extra: swap in a real model

**Folder:** `extras/real-model`

Extra credit: run it on the big screen, or after step 4.

The `model` slot is a label like any tool slot, so swapping the model is a
manifest change, from `model-scripted` to `model-real`. Nothing is rebuilt, and
the tools are unchanged.

`model-real` speaks the OpenAI chat completions API. `LLM_URL` points at the
fake endpoint in the toy sites (`llm.workshop-sites.svc`), so this still runs
offline. Only `model-real` is granted that host; no tool gets it.

**Run:**

```sh
kubectl apply -f extras/real-model/workload.yaml && sleep 2 && kubectl wait --for=condition=Ready workloaddeployment/toolbox-agent
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d 'what talks are there?'
```

**Expected:** the model asks for the `sqlite` tool, the agent runs it, and the
model answers from the result. The slot changed; the tools and their grants did
not.

**A real model, for real.** To point this at a hosted model or the workshop's
shared endpoint, change `LLM_URL` and `model`'s `allowedHosts` to that host,
and pass `LLM_API_KEY` from a Kubernetes Secret (`environment.secretFrom`). The
agent harness and the tools stay exactly as they are.

Full transcript: `expected-output.txt`.
