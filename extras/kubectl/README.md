# Extra: Give the agent kubectl

**Folder:** `extras/kubectl`

Extra credit: the session doesn't cover this one. It builds on step 3's
cluster state. Its manifest is `steps/03-sqlite/workload.yaml` with two
changes: the `kubectl` slot, which holds the placeholder in every step, gets
the real kubectl tool, and `hostInterfaces` gets the kube-api capability.

`kubectl` is the tool a platform team least wants an agent to have. Here the
agent gets it, and two layers decide what it can do: the wasmCloud grant and
Kubernetes RBAC (role-based access control, the API server's own rules).

The `kubectl` component holds no token and imports no HTTP client and no
sockets. Its only way to the cluster is `workshop:toolbox/kube-api`, served by
the **kube-api plugin**: a host component plugin, which is a component the
platform installs on the host group rather than a workload. The plugin holds
the token of a read-only identity, the `agent-reader` service account. That
token is shared by any workload on this host group that declares the plugin.
The plugin forwards any method and path it is given, and may reach only the
API server.

The tool takes ordinary kubectl command lines and answers as kubectl does:
`get` with kubectl's columns and `-o json`, `yaml`, `name` or `wide`,
`describe`, `logs`, `auth can-i`, `version`, and `get --raw` and
`delete --raw` for a request of your own. It only reads. Commands that need
a stream or a connection upgrade (`exec`, `port-forward`, `logs -f`) are
refused, because the capability carries one request and one answer.

## Two layers

| Layer | What it decides here |
| --- | --- |
| wasmCloud grant (the manifest and the host group) | Which components may call kube-api: only workloads that declare it under `hostInterfaces`, and in this workload only `kubectl` imports it. Where the plugin may connect: its `allowedHosts` is `kubernetes.default.svc`. Who holds the token: the plugin, never a workload. Every other tool keeps its own grant, so `curl` can't reach the API server. |
| Kubernetes RBAC (`k8s/kube.yaml`) | What `agent-reader` may do once a request arrives: `get` and `list` on pods, `pods/log`, Services, Deployments and WorkloadDeployments, in the `demo` namespace only. No Secrets, no ConfigMaps, no writes, no other namespace. |

## Deploy

```sh
kubectl apply -f extras/kubectl/workload.yaml \
  && sleep 2 && kubectl wait --for=condition=Ready \
  workloaddeployment/toolbox-agent
```

The same agent as step 3, with the real tool in its kubectl slot and one new
entry under `hostInterfaces`:

```yaml
- namespace: workshop
  package: toolbox
  interfaces: [kube-api]
```

## Try each command

Type each prompt into the chat at http://agent.localhost:18180, or run the
curl command under it. Each one is labelled with the layer that allowed or
refused it.

**1. Pods in demo** (allowed: grant and RBAC): `kubectl get pods`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'kubectl get pods'
```

kubectl's own columns, `NAME READY STATUS RESTARTS AGE`: the two `hello`
pods, `1/1 Running`. `demo` is this kubectl's default namespace.

**2. Deployments** (allowed: RBAC): `kubectl get deployments`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'kubectl get deployments'
```

`hello 2/2 2 2`: two of two pods ready, two up to date, two available.

**3. A pod's logs** (allowed: RBAC grants `pods/log`): `kubectl logs deployment/hello`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'kubectl logs deployment/hello'
```

`hello from hello-<pod>`: the tool reads the first of the deployment's pods.
Logs can carry sensitive data, so a real Role should grant `pods/log` only
where that is acceptable.

**4. Describe the deployment** (allowed: RBAC): `kubectl describe deployment hello`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'kubectl describe deployment hello'
```

kubectl's description, field for field: `Name: hello`, `Replicas: 2 desired
| 2 updated | 2 total | 2 available | 0 unavailable`, the pod template and
the conditions. Its last line is `Events: <none>`: agent-reader may not
list events, and real kubectl run as agent-reader prints the same. The
reply in the chat is cut at 300 characters; the model reads all of it.
`kubectl describe pods hello` describes every pod whose name starts with
`hello`.

**5. curl straight at the API server** (refused: the grant):
`curl -s https://kubernetes.default.svc/api/v1/namespaces/demo/pods`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'curl -s https://kubernetes.default.svc/api/v1/namespaces/demo/pods'
```

`access-denied: kubernetes.default.svc: this tool may not reach that host`.
The host refuses it because the API server isn't in curl's `allowedHosts`,
before RBAC is ever asked. curl holds no token either.

**6. The token itself** (refused: the grant):
`curl -s file:///etc/wasmcloud/host-plugin-secrets/kube-api-token/token`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'curl -s file:///etc/wasmcloud/host-plugin-secrets/kube-api-token/token'
```

`access-denied: /etc/wasmcloud/host-plugin-secrets/kube-api-token/token: No
such file or directory (this tool sees only /data)`. In a typical pod the
service account token is a file that any process can read, an agent's curl
included, and a stolen token works from anywhere that can reach the API
server. RBAC can't tell the thief from the owner. Here the file does exist
in the host pod, where the kube-api plugin reads it, but curl's filesystem
grant is `/data` alone, and kubectl never sees the token: it asks the plugin.
This token is a long-lived Secret token, the kind you'd never hand a process;
the platform holds it so no tool has to.

**7. Secrets** (refused: RBAC): `kubectl get secrets`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'kubectl get secrets'
```

The request reaches the API server, which answers `Forbidden`: agent-reader
cannot list secrets.

**8. Every namespace** (refused: RBAC): `kubectl get pods -A`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'kubectl get pods -A'
```

`Forbidden`: agent-reader may not list pods at the cluster scope.

**9. Another namespace** (refused: RBAC): `kubectl get pods -n kube-system`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'kubectl get pods -n kube-system'
```

`Forbidden`, the same way.

**10. Any method gets through the plugin** (refused: RBAC):
`kubectl delete --raw /api/v1/namespaces/demo/pods/hello`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'kubectl delete --raw /api/v1/namespaces/demo/pods/hello'
```

`--raw` sends exactly this request through the capability, past the tool's
own checks. The plugin forwards it, and the API server refuses it:
`Error from server (Forbidden): pods "hello" is forbidden: User
"system:serviceaccount:wasmcloud:agent-reader" cannot delete resource "pods"`.
The plugin is not a filter. RBAC is.

**11. Ask RBAC first** (answered by RBAC): `kubectl auth can-i list secrets`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'kubectl auth can-i list secrets'
```

`no`. The tool asks the API server what agent-reader may do, without trying
it. The same question about pods:

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'kubectl auth can-i list pods'
```

`yes`.

## Control: widen the Role

RBAC was the only thing refusing the secrets. Widen the Role to read them,
with nothing else changed:

```sh
kubectl apply -f extras/kubectl/control-secrets-role.yaml
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'kubectl get secrets'
```

Abbreviated, the tool's line of the reply (the age differs on every
cluster):

```
[tool kubectl] ok: NAME TYPE DATA AGE db-password Opaque 1 52m
```

Revert it right away:

```sh
kubectl apply -f k8s/kube.yaml
```

## What you ran

| Piece | Holds | Can reach |
| --- | --- | --- |
| `kubectl` tool | nothing | the kube-api capability, nothing else |
| kube-api plugin (platform) | agent-reader's token | the API server only (`allowedHosts`) |
| agent-reader (RBAC) | | `get`, `list` on pods, logs, Services, Deployments, WorkloadDeployments in `demo` |

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

## Production notes

This step is a minimal shape for a throwaway kind cluster, not a pattern to
copy as is.

* **The token is long-lived.** `kube-api-token` is a service-account-token
  Secret: it lasts until the Secret is deleted, and the host reads it once at
  startup. In production, use short-lived tokens from the TokenRequest API
  (projected service account tokens) and rotate them.
* **One identity per host group.** Every workload on this host group that
  declares kube-api acts as agent-reader. Give each trust domain its own host
  group and identity.
* **`httpClientCaPaths` is host-group wide.** Adding the cluster CA there makes
  every component and plugin on the host group trust it, not only kube-api.
* **Admission policy decides who may declare the plugin.** Any workload that
  may land on this host group can list `kube-api` under `hostInterfaces`. Use
  RBAC on WorkloadDeployments and an admission policy to decide which
  workloads may.
* **Address ranges.** wasmCloud v2.12.0 also has a host-wide range policy,
  `denySpecialRanges` in the chart (on by default), that refuses loopback,
  link-local and the cloud metadata address, including what DNS returns for a
  permitted name. In v2.12.0 it applies to raw sockets and native host
  plugins; for `wasi:http` requests, `allowedHosts` is the control. Keep it on.

Full transcript: [`expected-output.txt`](expected-output.txt). Pod names and
ages differ on every cluster; the transcript writes them as `hello-<pod>` and
`<age>`.
