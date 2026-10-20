# Step 7: two tenants on one host

**Folder:** `steps/07-tenants`

`tenants.yaml` deploys two copies of the step 6 workload, one per tenant,
each with its own ingress name (`a.agent.localhost`, `b.agent.localhost`),
keyvalue bucket and blobstore container. Separate workloads never share
component instances, mounted folders or `allowedHosts`.

**Run:**

```sh
python3 scripts/apply.py steps/07-tenants/tenants.yaml
curl -s -H 'Host: a.agent.localhost' http://127.0.0.1:18180/ -d 'remember that tenant is alpha'
curl -s -H 'Host: b.agent.localhost' http://127.0.0.1:18180/ -d 'remember that tenant is bravo'
curl -s -H 'Host: a.agent.localhost' http://127.0.0.1:18180/ -d 'what do you remember?'
curl -s -H 'Host: b.agent.localhost' http://127.0.0.1:18180/ -d 'what do you remember?'
```

**Expected:**

```
[tool recall_all] ok: tenant = alpha
[tool recall_all] ok: tenant = bravo
```

**Caveat: buckets are not scoped per workload.** On v2.10.3 the NATS keyvalue
backend opens whatever bucket a component names, on a connection shared by
all workloads. `caveat-shared-bucket.yaml` is tenant B's manifest pointing at
tenant A's bucket:

```sh
python3 scripts/apply.py steps/07-tenants/caveat-shared-bucket.yaml
curl -s -H 'Host: b.agent.localhost' http://127.0.0.1:18180/ -d 'what do you remember?'
```

```
[tool recall_all] ok: tenant = alpha
```

So isolation between tenants is only as strong as whoever writes the
manifests. A platform adds admission policy, a NATS account per tenant, or
operator declared bindings. The same goes for `localRoute` names: any
workload on a host can claim any name.

**Scale on Kubernetes.** On kind (`./scripts/kind-up.sh`), the workload is a
`WorkloadDeployment` with a `/scale` subresource:

```sh
KUBECONFIG=.run/kubeconfig kubectl scale workloaddeployment toolbox-agent --replicas=3
```

An HPA can drive the same subresource, but it needs an external metrics
adapter; there is no built in autoscaler.

Full transcript (including an `ab` load run): `expected-output.txt`.
