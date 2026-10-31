# Step 3: two teams, two namespaces, one host pod

**Folder:** `steps/03-tenants`

`tenants.yaml` creates two namespaces, `team-a` and `team-b`, and puts step
2's agent in each, with its own ingress name (`a.agent.localhost`,
`b.agent.localhost`) and its own store for its memory. Who may apply a
Workload into a team's namespace is ordinary Kubernetes RBAC.

**Run:**

```sh
kubectl apply -f steps/03-tenants/tenants.yaml && sleep 2
kubectl wait --for=condition=Ready workloaddeployment/toolbox-agent -n team-a
kubectl wait --for=condition=Ready workloaddeployment/toolbox-agent -n team-b
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

**In a browser:** open http://a.agent.localhost:18180 and
http://b.agent.localhost:18180 in two tabs. Each team's agent serves its own
Agent chat page, which sends prompts only to that team's agent, so you can
run the same four prompts side by side. That works on your own machine; in
Codespaces, use the curl commands.

Both teams wrote the same key, `our team`, and each still reads its own
value: neither can read or overwrite the other's.

Every agent in the cluster runs on the same host id: one host pod, many
tenants. That is the point of a wasmCloud host: a pod that runs many small,
separate workloads, each with its own memory and only its own grants, instead
of one pod per agent.

**Its own store.** The data service is one Valkey server (an open source fork
of Redis). Valkey has no buckets, so the platform splits it into stores by
key prefix, in `k8s/values.yaml`:

```yaml
plugins:
  - id: wasmcloud-keyvalue-multiplexed
    workloadConfig: deny
    hostOwnedKeys: [backend]
    config:                     # the default store (steps 1, 2, 5 to 8)
      backend: redis
      url: redis://valkey.workshop-data.svc:6379
      prefix: "agent:"
    bindings:                   # one store per team
      team-a: { config: { prefix: "team-a:" } }
      team-b: { config: { prefix: "team-b:" } }
```

A workload asks for a store by name. Each team's manifest names its keyvalue
host interface after the team (`name: team-a`), and its memory tool is step
2's code built with its store import labelled the same way
(`import team-a: wasmcloud:keyvalue/store`, in the `tool-memory-team-a`
image). When team-a's tool writes `our_team`, the key in Valkey is
`team-a:our_team`:

```sh
kubectl exec -n workshop-data deploy/valkey -- valkey-cli --scan --pattern 'team-*'
```

```
team-a:our_team
team-b:our_team
```

Three things keep a team inside its own store:

* `workloadConfig: deny`: the address, the prefix and the kind of backend
  come only from the platform. A manifest that sets one is refused when the
  host binds it (`own-prefix.yaml` tries).
* A store's name is host wide, not tied to a namespace: any workload that
  asks for `team-a` would get team-a's store. So the platform adds an
  admission policy (`k8s/tenant-policy.yaml`, built into Kubernetes) that
  lets a team ask only for the store named after its namespace. team-b asking
  for `team-a` is refused at `kubectl apply` (`other-store.yaml` tries).
* The same policy keeps the teams on the default host group, where their
  stores are declared.

`make prove` runs these checks and a restart of the host pod and of Valkey
(team-a's facts are still there afterwards): `control-checks.txt`.

**One isolation layer.** Upstream wasmCloud's threat model says it plainly:
workloads on one host are separated by the WebAssembly runtime alone. For
tenants who are actively hostile to each other, give them separate host
groups or nodes, and decide with RBAC and admission policy who may apply
which Workloads.

**Why the toy sites live on their own host group.** Any workload can claim
any hostname on its host. Inside one host group, the chart's local routing
would deliver a request for a name to whichever workload on that host
claimed it, so team-b could claim `llm.workshop-sites.svc` and read team-a's
traffic to the model in step 7. So local routing is off, and the toy sites
run on a host group of their own behind Kubernetes Services. `make prove`
checks this: a team-b workload that claims their names (`squatter.yaml`)
catches none of team-a's requests (`control-checks.txt`).

Full transcript: `expected-output.txt`.
