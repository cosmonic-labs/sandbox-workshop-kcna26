# Fixtures: the toy sites

Not a step. `make up` starts this workload, which serves several fake websites
from one component, all inside the cluster, so the workshop needs no internet:

* `api.workshop-sites.svc/data`: an ordinary page, the one host the `curl` tool
  is granted in the steps. `/redirect` returns a 302 to the collector, a host
  `curl` is not granted, so step 3 can show the host checks each hop of a
  redirect.
* `collector.workshop-sites.svc/collect`: a stand-in for an attacker's server.
  It counts every request that reaches it, in the keyvalue bucket
  `toy-collector`.
* `planted.workshop-sites.svc/page`: a normal looking page with a hidden
  instruction, used by the container demo in the presentation
  (`extras/container`).
* `llm.workshop-sites.svc/v1/chat/completions`: a fake OpenAI compatible model
  for the real-model extra (`extras/real-model`).

Each name is an ordinary Kubernetes Service (`k8s/sites.yaml`, namespace
`workshop-sites`) that selects the pods of the `fixtures` host group, and the
workload asks for that host group with `hostSelector`. The host sends each
request to the workload that serves its `Host` header, so `toy-sites.yaml` lists
the names under `host-aliases`. A caller still needs the name in its own
`allowedHosts`.

Your laptop uses `sites.localhost` through the ingress, which serves the same
pages and collector. Only `sites.localhost` may reset or list the leaks (`POST
/reset`, `GET /leaks`); `/count` answers on any name.

```sh
curl -s -H 'Host: sites.localhost' http://127.0.0.1:18180/count    # leaks so far
curl -s -X POST -H 'Host: sites.localhost' http://127.0.0.1:18180/reset
```

In a browser, http://sites.localhost:18180 shows each leak as it arrives.
