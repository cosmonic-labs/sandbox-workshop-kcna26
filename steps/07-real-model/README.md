# Step 7: swap in a real model

**Folder:** `steps/07-real-model`

The `model` slot is a label like any tool slot, so swapping the model is a
manifest change: the component named `model` now runs the `model-real` image,
which speaks the OpenAI chat completions API. Nothing is rebuilt. Only
`model-real` may reach the model endpoint (`allowedHosts: [llm.example]`); no
tool gets that host.

To stay offline, `LLM_URL` points at `llm.example`, the fake endpoint in the
toy sites. For a hosted model, set `LLM_URL` (for example
`https://api.openai.com/v1/chat/completions`), put that host in
`allowedHosts`, and pass `LLM_API_KEY` from a Kubernetes Secret
(`environment.secretFrom`).

**Run:**

```sh
kubectl apply -f steps/07-real-model/workload.yaml
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d 'hello'
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d 'recall what you know about me'
```

**Expected:**

```
[model] (fake-llm via llm.example) I got 2 messages and 8 tools

[tool recall_all] ok: my favorite color = teal; my api key = sk-demo-123
[model] (fake-llm) the tool said: my favorite color = teal; my api key = sk-demo-123
```

A real model will not reliably follow planted text, which is why the attack
steps use the scripted one. The sandbox does not depend on the model
behaving: steps 5 and 6 held against a model that obeys everything.

Full transcript: `expected-output.txt`.
