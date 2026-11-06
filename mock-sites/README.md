# Mock sites

`make up` starts this workload, which serves two mock sites from
one component, inside the cluster:

* `api.mock-sites.svc/data`: an ordinary API, the one host the `curl` tool is
  granted in the steps. `/redirect` returns a 302 to the attacker's server, a
  host `curl` is not granted, so step 2 can show the host checks each hop of a
  redirect.
* `attacker.mock-sites.svc/collect`: a mock malicious server. It stands in
  for the server of someone trying to hijack the agent with a prompt
  injection: an instruction planted in a web page, a database row or an issue
  comment, telling the agent to send its sensitive data to this address. It
  answers whatever reaches it, so when the host refuses a request to it, the
  agent's grant is the only thing in the way.

Each name is an ordinary Kubernetes Service (`k8s/sites.yaml`, namespace
`mock-sites`) that selects the pods of the `mock-sites` host group, and the
workload asks for that host group with `hostSelector`. The host sends each
request to the workload that serves its `Host` header, so
`workloaddeployment.yaml` lists both names. A caller still needs the name in
its own `allowedHosts`.
