# The demo websites

Not a step. `make up` starts this workload, which serves two fake websites from
one component, inside the cluster:

* `api.demo-sites.svc/data`: an ordinary API, the one host the `curl` tool is
  granted in the steps. `/redirect` returns a 302 to the attacker's server, a
  host `curl` is not granted, so step 2 can show the host checks each hop of a
  redirect.
* `attacker.demo-sites.svc/collect`: a stand-in for an attacker's server. It
  is on the network and answers whatever reaches it, so when the host refuses
  a request to it, the grant is the only thing in the way.

Each name is an ordinary Kubernetes Service (`k8s/sites.yaml`, namespace
`demo-sites`) that selects the pods of the `demo-sites` host group, and the
workload asks for that host group with `hostSelector`. The host sends each
request to the workload that serves its `Host` header, so `demo-sites.yaml`
lists both names. A caller still needs the name in its own `allowedHosts`.
