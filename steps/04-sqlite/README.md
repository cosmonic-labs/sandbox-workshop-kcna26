# Step 4: GTFOBins, sqlite3

**Folder:** `steps/04-sqlite`

`sqlite3` is on [GTFOBins](https://gtfobins.github.io/) for its `.shell` and
`.system` dot-commands, which start a shell, for `ATTACH` to any path on disk,
and for `load_extension`, which loads native code. Here there is no process to
start, the only files are in the one folder the manifest mounts read only, and
extensions are off. The tool reads the talks database at `/data/main.db`.

**Run:**

```sh
kubectl apply -f steps/04-sqlite/workload.yaml && sleep 2 && kubectl wait --for=condition=Ready workloaddeployment/toolbox-agent

# Ordinary SQL works
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d "sqlite select title, speaker from talks where track = 'Security' order by title"
# Dot-commands work
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d 'sqlite .tables'
# .shell cannot exist
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d 'sqlite .shell id'
# ATTACH outside the folder: refused
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d "sqlite attach '/etc/passwd' as evil"
# load_extension: not available
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d "sqlite select load_extension('/tmp/evil')"
```

**Expected:** the queries and `.tables` return rows. `.shell` comes back
"there is no shell here. This sqlite3 runs as a component that can't start
processes." `ATTACH` to `/etc/passwd` is refused because the tool sees only its
folder, and `load_extension` is not a function. The escapes have nothing to
escape to.

Full transcript: `expected-output.txt`.
