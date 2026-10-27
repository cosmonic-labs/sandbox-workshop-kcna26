# Step 5: swap in a real model

**Folder:** `steps/05-real-model`

The `model` slot is a label like any tool slot, so swapping the model is a
manifest change: the component named `model` now runs the `model-real` image,
which speaks the OpenAI chat completions API. Nothing is rebuilt. Only
`model-real` is allowed to reach the model endpoint (`allowedHosts:
[llm.example]`); no tool gets that host.

To stay offline, `LLM_URL` points at `llm.example`, a fake endpoint served by
the toy sites. For a hosted model, set `LLM_URL` (for example
`https://api.openai.com/v1/chat/completions`), put that host in
`allowedHosts`, and pass `LLM_API_KEY` from a Secret (`environment.secretFrom`
on a cluster).

Swap back to `model-scripted` before step 6: a real model does not obey a
planted instruction reliably, and the demo needs the hijack to succeed.

**Run:**

```sh
python3 scripts/apply.py steps/00-fixtures/toy-sites.yaml
python3 scripts/apply.py steps/05-real-model/workload.yaml
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d 'hello'
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d 'recall what you know about me'
```

**Expected:**

```
[user] hello
[model] (fake-llm via llm.example) I got 2 messages and 8 tools

[user] recall what you know about me
[model] call recall_all({})
[tool recall_all] ok: my favorite color = teal; my api key = sk-demo-123
[model] (fake-llm) the tool said: my favorite color = teal; my api key = sk-demo-123
```

"8 tools" is the eight rows of the harness's translation table, sent to the
model as JSON tool definitions.

Full transcript: `expected-output.txt`.
