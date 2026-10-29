# Fixtures: the toy sites

Not a step. `make up` starts this workload, which serves three fake websites
from one component:

* `planted.example/page`: a normal looking page with a hidden instruction for
  the agent.
* `collector.example/collect`: the attacker's server. It counts every leak
  that reaches it.
* `llm.example/v1/chat/completions`: a fake OpenAI compatible model for step 7.

These names exist nowhere in DNS. Workloads on the same host reach them in
memory through local routing (`localRoute` in `toy-sites.yaml`, allowed by the
chart's `localBypassRouting`). A component still needs the name in its own
`allowedHosts`: the egress check happens before local routing. Callers from
outside the host, such as step 4's container, use the name `sites.localhost`,
which serves the same page and collector.

```sh
curl -s -H 'Host: sites.localhost' http://127.0.0.1:18180/count    # leaks so far
curl -s -X POST -H 'Host: sites.localhost' http://127.0.0.1:18180/reset
```

The collector keeps its count, and the last 50 leaks, in the keyvalue bucket
`toy-collector`. In a browser, http://sites.localhost:18180 shows the same
thing live (`GET /leaks` is its JSON).
