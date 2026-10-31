# Step 8: scale across hosts, then troubleshoot

**Folder:** `steps/08-scale`

So far one host pod has run every agent. In this step you give the default
host group a second host pod, run the agent on several replicas, and watch
where the operator puts them. Then you break the workload three ways and
find out why each one fails, using only `kubectl`.

Start from step 7's workload (apply it again if you skipped ahead):

```sh
kubectl apply -f steps/07-real-model/workload.yaml && sleep 2 && kubectl wait --for=condition=Ready workloaddeployment/toolbox-agent
```

## Scale out

A host group is an ordinary Deployment of host pods. Give it a second one,
then ask for three replicas of the agent:

```sh
kubectl -n wasmcloud scale deployment hostgroup-default --replicas=2
kubectl -n wasmcloud rollout status deployment/hostgroup-default
kubectl get hosts -n wasmcloud
kubectl scale workloaddeployment toolbox-agent --replicas=3
sleep 2 && kubectl wait --for=condition=Ready workloaddeployment/toolbox-agent
kubectl get workloads -n default -o custom-columns=NAME:.metadata.name,HOST:.status.hostId
```

**Expected:** two hosts in the `default` host group (and the toy sites' host
in `fixtures`), and three workloads spread over the two default host ids.
The operator picks a host at random among the ones the `hostSelector`
matches, so your split may be 2 and 1, or all three on one host.

```
NAME                     HOSTID                                 HOSTGROUP   ...
determined-step-3745     01a18d40-5299-...                      default
next-toy-6856            01a18930-6cea-...                      default
material-building-3033   01a18930-6b90-...                      fixtures

NAME                                  HOST
toolbox-agent-6559f9c66f-56c4c6d58f   01a18930-6cea-...
toolbox-agent-6559f9c66f-57dc64fd96   01a18d40-5299-...
toolbox-agent-6559f9c66f-68887ff79    01a18930-6cea-...
```

**How requests find a replica.** The platform created a Service named
`toolbox-agent` with no selector (`k8s/agent-service.yaml`), and every
step's WorkloadDeployment names it under `spec.kubernetes.service`. The operator keeps that Service's
endpoints pointed at exactly the host pods that run a ready replica:

```sh
kubectl get endpointslices -l kubernetes.io/service-name=toolbox-agent
for i in 1 2 3 4 5 6; do curl -s -o /dev/null -w '%{http_code} ' -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d 'hello'; done; echo
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d 'recall what you know about me'
```

```
NAME                     ADDRESSTYPE   PORTS   ENDPOINTS                 AGE
toolbox-agent-9dda31f4   IPv4          80      10.244.0.4,10.244.0.16    1m
200 200 200 200 200 200
[tool recall_all] ok: my favorite color = teal; my api key = sk-demo-123
```

Every request is answered, whichever replica takes it, and every replica
sees the same memories, because they live in the data service and not in
any replica. If the ingress sent requests to the host group's own Service
instead, a request that landed on a host pod without a replica would get a
404. That's why the workshop routes through the operator's Service.

**Scale back in** and watch the operator move replicas off the host pod
that goes away:

```sh
kubectl -n wasmcloud scale deployment hostgroup-default --replicas=1
kubectl -n wasmcloud rollout status deployment/hostgroup-default
sleep 5 && kubectl get workloads -n default -o custom-columns=NAME:.metadata.name,HOST:.status.hostId
kubectl scale workloaddeployment toolbox-agent --replicas=1
```

All three replicas now report the one remaining host id. On a real cluster,
set the host group's size in the chart's values
(`runtime.hostGroups[].replicas` in `k8s/values.yaml`), or the next
`helm upgrade` (here, `make up`) puts it back to one.

## Troubleshoot

Each manifest in this folder is step 7's workload with one thing broken.
Apply one, find out what's wrong, then put step 7 back before the next.

```sh
kubectl apply -f steps/08-scale/broken-interface.yaml
sleep 2 && kubectl wait --for=condition=Ready workloaddeployment/toolbox-agent --timeout=30s
```

The wait times out, yet `curl` still answers. A broken version never takes
over: the previous one keeps serving until the new one is ready. So the
question is why the new one isn't. Ask the Workloads:

```sh
kubectl describe workloads -n default | grep -E '^Name:|Message:'
```

```
Message: workload is not operational: WORKLOAD_STATE_ERROR: failed to pre-instantiate during component linking: component imports instance `wasmcloud:keyvalue/types@0.2.0`, but a matching implementation was not found in the linker: ...
```

The memory component imports keyvalue, but the manifest no longer lists it
under `hostInterfaces`, so the host has nothing to link it to. A component
gets only what the workload asks for, and a missing grant fails at startup,
before any request.

Put step 7 back, then try the next:

```sh
kubectl apply -f steps/07-real-model/workload.yaml && sleep 2 && kubectl wait --for=condition=Ready workloaddeployment/toolbox-agent
kubectl apply -f steps/08-scale/broken-image.yaml
sleep 2 && kubectl wait --for=condition=Ready workloaddeployment/toolbox-agent --timeout=30s
kubectl describe workloads -n default | grep -E '^Name:|Message:'
```

This time the message only says `WORKLOAD_STATE_NOT_FOUND: Workload not
found`. The reason is in the host pod's log:

```sh
kubectl logs -n wasmcloud deployment/hostgroup-default --since=5m | grep 'failed to start'
```

```
... failed to start workload ... reason="failed to pull image for component 'memory': failed to pull component from workshop-toolbox-registry:5000/workshop/tool-memroy:0.4.0: ...
```

The last one asks for a keyvalue store the platform never declared:

```sh
kubectl apply -f steps/07-real-model/workload.yaml && sleep 2 && kubectl wait --for=condition=Ready workloaddeployment/toolbox-agent
kubectl apply -f steps/08-scale/broken-store.yaml
sleep 2 && kubectl wait --for=condition=Ready workloaddeployment/toolbox-agent --timeout=30s
kubectl describe workloads -n default | grep -E '^Name:|Message:'
```

```
Message: workload is not operational: WORKLOAD_STATE_ERROR: workload ... cannot bind plugin 'wasmcloud-keyvalue-multiplexed': this plugin declares bindings; it serves `team-a`, `team-b` and `agent-memory` is not among them; ...
```

The manifest names its keyvalue host interface `agent-memory`. The platform
declares a store per name (`k8s/values.yaml`, `bindings:`), and a workload
can ask only for one of those, so a misspelled or invented name fails at
startup instead of quietly opening an empty store. The message lists the
names that exist.

Put step 7 back when you're done:
`kubectl apply -f steps/07-real-model/workload.yaml && sleep 2 && kubectl wait --for=condition=Ready workloaddeployment/toolbox-agent`.

## What to check when something's off

| You see | What it means | Look at |
|---|---|---|
| The previous step's output right after an apply | The new version isn't running yet | Wait: `sleep 2 && kubectl wait --for=condition=Ready ...` |
| `kubectl wait` times out, but `curl` still answers | The new version failed; the old one keeps serving | `kubectl describe workloads -n <ns>`, the `Message:` lines |
| The message says `Workload not found` | The host couldn't start it | `kubectl logs -n wasmcloud deployment/hostgroup-default` |
| A tool error such as `access-denied` or `Redis error` | The workload runs, but a grant is wrong or the data service is down | The tool's message, then the manifest's `allowedHosts` or volumes, or `kubectl get pods -n workshop-data` |
| HTTP 502 from `curl` | No ready replica behind that hostname yet | `kubectl get endpointslices -n <ns>`, and whether the step was applied |
| HTTP 404 from `curl` | No workload claims that hostname | The `Host:` header, and the manifest's `host:` |

Full transcript: `expected-output.txt`.
