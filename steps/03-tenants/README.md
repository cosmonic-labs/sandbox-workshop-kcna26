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

NAMESPACE   HOST
default     01a17e54-...
default     01a17e54-...
team-a      01a17e54-...
team-b      01a17e54-...
```

Every workload in the cluster runs on the same host id: one host pod, many
tenants. That is the point of a wasmCloud host: a pod that runs many small,
separate workloads, each with its own memory and only its own grants, instead
of one pod per agent. (Running a very large number of agents at once is
coming up in the session itself.)

**One isolation layer.** Upstream wasmCloud's threat model says it plainly:
workloads on one host are separated by the WebAssembly runtime alone. For
tenants who are actively hostile to each other, give them separate host
groups or nodes, and decide with RBAC and admission policy who may apply
which Workloads.

Full transcript: `expected-output.txt`.
