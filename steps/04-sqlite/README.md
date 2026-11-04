# Step 4: Now try sqlite3's escapes

**Folder:** `steps/04-sqlite`

`sqlite3` is on [GTFOBins](https://gtfobins.github.io/) for `.shell`, which
starts a shell, for `ATTACH` to any file on disk, and for `load_extension`,
which loads native code. Here there is no process to start, the only files are
in the one folder the manifest mounts read only, and extensions are off.

## Deploy

```sh
kubectl apply -f steps/04-sqlite/workload.yaml \
  && sleep 2 && kubectl wait --for=condition=Ready \
  workloaddeployment/toolbox-agent
```

The grant: the talks database, read only, and nothing else.

```yaml
volumeMounts: [{ name: sqlite-data, mountPath: /data, readOnly: true }]
```

## Try each command

Type each prompt into the chat at http://agent.localhost:18180, or run the curl command under it.

**1. Ordinary SQL works:** `sqlite select title, speaker from talks where track = 'Security' order by title`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d "sqlite select title, speaker from talks where track = 'Security' order by title"
```

This is a real sqlite3, reading the talks database.

**2. Dot-commands work:** `sqlite .tables`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'sqlite .tables'
```

The sqlite3 shell's own commands work too. Next, the ones on GTFOBins.

**3. .shell has nothing to start:** `sqlite .shell id`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'sqlite .shell id'
```

The tool answers "there is no shell here. This sqlite3 runs as a component that can't start processes."

**4. ATTACH can't reach /etc/passwd:** `sqlite attach '/etc/passwd' as evil`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d "sqlite attach '/etc/passwd' as evil"
```

Refused with `access-denied`: this tool sees only its folder, so `/etc/passwd` doesn't exist for it.

**5. load_extension doesn't exist:** `sqlite select load_extension('/tmp/evil')`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d "sqlite select load_extension('/tmp/evil')"
```

"no such function": it isn't compiled in, and there is no native code to load.

Full transcript: [`expected-output.txt`](expected-output.txt).
