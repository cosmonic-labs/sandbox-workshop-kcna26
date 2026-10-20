# Fixtures: the toy sites

Not a step. This workload serves three fake websites from one component so
steps 5 and 6 can run fully offline:

* `planted.example/page`: a normal looking page with a hidden instruction for
  the agent.
* `collector.example/collect`: the attacker's server. It counts every leak
  that reaches it.
* `llm.example/v1/chat/completions`: a fake OpenAI compatible model for step 5.

These names exist nowhere in DNS. The host routes them in memory to this
workload (`localRoute`, needs `wash host --http-local-routing`, which
`scripts/up.sh` sets). A component still needs the name in its
`allowedHosts` to reach them: local routing happens after the allowlist check.

**Run** (from the repo root, after `./scripts/up.sh` and `./scripts/push.sh artifacts`):

```sh
python3 scripts/apply.py steps/00-fixtures/toy-sites.yaml
curl -s -H 'Host: sites.localhost' http://127.0.0.1:18180/count
```

**Expected:**

```
default-toy-sites: WORKLOAD_STATE_RUNNING Workload started successfully
leaks=0
last=
```

Reset the counter with `curl -s -X POST -H 'Host: sites.localhost' http://127.0.0.1:18180/reset`.
