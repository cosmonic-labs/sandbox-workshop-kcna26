# Step 6: Give the agent kubectl

**Folder:** `steps/06-kubectl`

`kubectl` is the tool a platform team least wants an agent to have. Here the
agent gets it, and two things still decide what it can do: the capability the
platform gives it, and Kubernetes RBAC.

The `kubectl` component holds no token and imports no HTTP client and no
sockets. Its only way to the cluster is `workshop:toolbox/kube-api`, served by
the **kube-api plugin**: a host component plugin (a component the platform
installs on the host group, not a workload) that holds the `agent-reader`
service account's token and may reach only the API server. RBAC on
`agent-reader` allows reading pods, logs, Services, ConfigMaps, Deployments
and WorkloadDeployments in the `demo` namespace, and nothing else.

## Deploy

```sh
kubectl apply -f steps/06-kubectl/workload.yaml \
  && sleep 2 && kubectl wait --for=condition=Ready \
  workloaddeployment/toolbox-agent
```

The same agent as step 1, with the real tool in its fourth slot and one new
line under `hostInterfaces`:

```yaml
- namespace: workshop
  package: toolbox
  interfaces: [kube-api]
```

## Try each command

Type each prompt into the chat at http://agent.localhost:18180, or run the curl command under it.

**1. Pods in demo:** `kubectl get pods`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'kubectl get pods'
```

The two `hello` pods, `Running`. demo is the namespace agent-reader may read, and this kubectl's default.

**2. Deployments:** `kubectl get deployments`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'kubectl get deployments'
```

`hello`, `2/2 ready`.

**3. A pod's logs:** `kubectl logs <pod>`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'kubectl logs <pod>'
```

Use a pod name from item 1. The pod prints `hello from <pod>`.

**4. Secrets are refused:** `kubectl get secrets`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'kubectl get secrets'
```

`access-denied`: the API server says agent-reader cannot list secrets. RBAC still applies.

**5. Every namespace is refused:** `kubectl get pods -A`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'kubectl get pods -A'
```

`access-denied`: agent-reader may not list pods at the cluster scope.

**6. Other namespaces are refused:** `kubectl get pods -n kube-system`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'kubectl get pods -n kube-system'
```

`access-denied`, the same way.

## What you ran

| Piece | Holds | Can reach |
| --- | --- | --- |
| `kubectl` tool | nothing | the kube-api capability, nothing else |
| kube-api plugin (platform) | agent-reader's token | the API server only (`allowedHosts`) |
| agent-reader (RBAC) | | read pods, logs, Services, ConfigMaps, Deployments in `demo` |

The tool imports the capability and no HTTP client, sockets or secrets:

```sh
wasm-tools component wit artifacts/tool_kubectl.wasm \
  | grep -E '^  import (workshop|wasi:http|wasi:sockets|wasmcloud)'
```

```
  import workshop:toolbox/kube-api@0.4.0;
```

The platform side is in `k8s/values.yaml` (the plugin, its `allowedHosts`, the
token Secret it reads, and the cluster CA it trusts) and `k8s/kube.yaml` (the
`demo` namespace, `agent-reader`, its token and its Role).

This is a minimal shape, not full credential brokering: one token for the
whole host group, read when the host starts.

Full transcript: [`expected-output.txt`](expected-output.txt). Pod names differ
on every cluster.
