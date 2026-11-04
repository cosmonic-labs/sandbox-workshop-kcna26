# Step 3: Now try to make curl escape

**Folder:** `steps/03-curl`

`curl` is on [GTFOBins](https://gtfobins.github.io/) because it reaches places
you did not mean it to: the cloud metadata service, other hosts, and anywhere
on disk with `-o`. Here every request goes through `wasi:http`, where the host
checks it against this component's `allowedHosts` before anything leaves, and
files exist only in the one folder the manifest mounts.

## Deploy

```sh
kubectl apply -f steps/03-curl/workload.yaml \
  && sleep 2 && kubectl wait --for=condition=Ready \
  workloaddeployment/toolbox-agent
```

Same grant as step 2: one host, one folder.

```yaml
allowedHosts: [api.workshop-sites.svc]
volumeMounts: [{ name: curl-data, mountPath: /data }]
```

## Try each escape

Type each prompt into the chat at http://agent.localhost:18180, or run the curl command under it.

**1. The granted host answers:** `curl -s http://api.workshop-sites.svc/data`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'curl -s http://api.workshop-sites.svc/data'
```

A baseline: the one host curl may reach.

**2. The metadata service is refused:** `curl -s http://169.254.169.254/latest/meta-data/`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'curl -s http://169.254.169.254/latest/meta-data/'
```

On most clouds this address hands out credentials. The host refuses it with `access-denied` before anything leaves.

**3. Other cluster hosts are refused:** `curl -s http://collector.workshop-sites.svc/collect`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'curl -s http://collector.workshop-sites.svc/collect'
```

The collector stands in for an attacker's server. It is on the network, but not in curl's grant: `access-denied`.

**4. A redirect can't sneak past:** `curl -sL http://api.workshop-sites.svc/redirect`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'curl -sL http://api.workshop-sites.svc/redirect'
```

The granted host redirects to the collector. The host checks every hop, including the one you didn't type: `access-denied`.

**5. Writing outside /data fails:** `curl -o /etc/evil -s http://api.workshop-sites.svc/data`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'curl -o /etc/evil -s http://api.workshop-sites.svc/data'
```

This curl sees one folder, so `/etc` doesn't exist for it.

**6. Writing inside /data works:** `curl -o page.html -s http://api.workshop-sites.svc/data`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'curl -o page.html -s http://api.workshop-sites.svc/data'
```

The grant includes `/data`, so the write succeeds. The folder is the boundary, not a blanket ban.

**The grant is the only thing stopping it.** `make prove` also runs a control
with `allowedHosts` widened to `*`, and the same request to the collector then
goes through (`control-checks.txt`). The tool didn't change; only its grant did.

Full transcript: [`expected-output.txt`](expected-output.txt).
