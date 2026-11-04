# Step 3: GTFOBins, curl

**Folder:** `steps/03-curl`

`curl` is on [GTFOBins](https://gtfobins.github.io/) because it reaches places
you did not mean it to: the cloud metadata service, your own loopback services,
`file://` URLs, and anywhere on disk with `-o`. Here every request goes through
`wasi:http`, where the host checks it against this component's `allowedHosts`
before anything leaves, and files exist only in the one folder the manifest
mounts. The tool is granted one host, `api.workshop-sites.svc`.

**Run:**

```sh
kubectl apply -f steps/03-curl/workload.yaml && sleep 2 && kubectl wait --for=condition=Ready workloaddeployment/toolbox-agent

# Granted: the api host answers
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d 'curl -s http://api.workshop-sites.svc/data'
# The cloud metadata service: refused
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d 'curl -s http://169.254.169.254/latest/meta-data/'
# Another cluster host: refused
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d 'curl -s http://collector.workshop-sites.svc/collect'
# A redirect from the granted host to a denied host: refused on the second hop
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d 'curl -sL http://api.workshop-sites.svc/redirect'
# Write outside the tool's folder: refused
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d 'curl -o /etc/evil -s http://api.workshop-sites.svc/data'
# Write inside the tool's folder: allowed
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d 'curl -o page.html -s http://api.workshop-sites.svc/data'
# Read a file:// URL outside the tool's folder: refused
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d 'curl -s file:///etc/passwd'
# Read a file:// URL inside the tool's folder: allowed
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d 'curl -s file:///data/page.html'
```

**Expected:** the granted host answers. The metadata service, the other host
and the second hop of the redirect all come back as `access-denied` from the
host's grant check. Writing to `/etc/evil` and reading `file:///etc/passwd` are both
refused because the tool sees only `/data`; writing `page.html` into `/data`
and reading it back with `file://` work. The tool does not check file paths
itself: the filesystem it is given holds only that one folder. The redirect is the sharpest
one: the host checks the hop you did not type.

**The grant is the only thing stopping it.** `make prove` runs a control with
`allowedHosts` widened to `*`, and the same request to the collector then goes
through (`control-checks.txt`). Nothing about the tool changed; only its grant
did.

Full transcript: `expected-output.txt`.
