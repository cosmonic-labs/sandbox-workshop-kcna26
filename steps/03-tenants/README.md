# Step 3: two teams, two namespaces, one host pod

**Folder:** `steps/03-tenants`

`tenants.yaml` creates two namespaces, `team-a` and `team-b`, and puts step
2's agent in each, with its own ingress name (`a.agent.localhost`,
`b.agent.localhost`) and its own memory bucket. Nothing else changes. Who may
apply a Workload into a team's namespace is ordinary Kubernetes RBAC.

**Run:**

```sh
kubectl apply -f steps/03-tenants/tenants.yaml
curl -s -H 'Host: a.agent.localhost' http://127.0.0.1:18180/ -d 'remember that our team is alpha'
curl -s -H 'Host: b.agent.localhost' http://127.0.0.1:18180/ -d 'remember that our team is bravo'
curl -s -H 'Host: a.agent.localhost' http://127.0.0.1:18180/ -d 'what do you remember?'
curl -s -H 'Host: b.agent.localhost' http://127.0.0.1:18180/ -d 'what do you remember?'
kubectl get workloads -A -o custom-columns=NAMESPACE:.metadata.namespace,HOST:.status.hostId
```

**Expected:**

```
[tool recall_all] ok: our team = alpha
[tool recall_all] ok: our team = bravo

NAMESPACE        HOST
default          01a1890d-29d0-...
team-a           01a1890d-29d0-...
team-b           01a1890d-29d0-...
workshop-sites   01a1890d-6df6-...
```

Every agent in the cluster runs on the same host id: one host pod, many
tenants. That is the point of a wasmCloud host: a pod that runs many small,
separate workloads, each with its own memory and only its own grants, instead
of one pod per agent.

**One isolation layer.** Upstream wasmCloud's threat model says it plainly:
workloads on one host are separated by the WebAssembly runtime alone. For
tenants who are actively hostile to each other, give them separate host
groups or nodes, and decide with RBAC and admission policy who may apply
which Workloads.

**Why the toy sites live on their own host group.** A workload can claim any
hostname on its host, and with the chart's local routing on, the host would
hand it its neighbors' requests to that name, so team-b could read team-a's
traffic to the model in step 7. So local routing is off and the toy sites run
on a host group of their own behind Kubernetes Services; `make prove` checks
that a team-b workload claiming their names (`squatter.yaml`) catches none of
team-a's requests.

Full transcript: `expected-output.txt`.
