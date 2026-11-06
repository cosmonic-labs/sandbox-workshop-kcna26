# Step 3: Give the agent sqlite, then try its escapes

**Folder:** `steps/03-sqlite`

`sqlite3` is on [GTFOBins](https://gtfobins.github.io/) for three things:
`.shell`, which starts a shell; `ATTACH`, which opens any database file on
disk; and `load_extension`, which loads native code.

The sqlite tool is the real SQLite engine with a minimal shell around it. The
engine runs your SQL as it is. The shell is the tool's own small code: it
turns `.tables` and `.schema` into SQL and answers `.shell` itself.

## Deploy

```sh
kubectl apply -f steps/03-sqlite/workload.yaml \
  && sleep 2 && kubectl wait --for=condition=Ready \
  workloaddeployment/toolbox-agent
```

The same agent as step 2, with the real sqlite tool in its slot. Its grant:
one folder, read only, holding two databases (`main.db`, the talks, and
`rooms.db`).

```yaml
volumeMounts: [{ name: sqlite-data, mountPath: /data, readOnly: true }]
```

The host pod also has a third database, `/workshop/private-data/payroll.db`,
on its own disk. Native sqlite3 on that machine would open it. No workload is
granted that folder, and the host's `hostPaths` list doesn't allow it, so no
workload may even ask for it. See it on the host pod:

```sh
kubectl -n wasmcloud exec deploy/hostgroup-default -- ls -l /workshop/private-data
```

```
-rw-r--r--    1 nonroot  nonroot      12288 Nov  5 01:13 payroll.db
```

## Try each command

Type each prompt into the chat at http://agent.localhost:18180, or run the
curl command under it.

**1. Ordinary SQL works:** `sqlite select title, speaker from talks where track = 'Security' order by title`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d "sqlite select title, speaker from talks where track = 'Security' order by title"
```

The SQLite engine reads the talks database.

**2. A dot-command works:** `sqlite .tables`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'sqlite .tables'
```

The tool's shell turns `.tables` into a query on SQLite's schema table.

**3. ATTACH a database inside the grant: it works**

`sqlite attach 'file:/data/rooms.db?immutable=1' as r; select name, seats from r.rooms`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d "sqlite attach 'file:/data/rooms.db?immutable=1' as r; select name, seats from r.rooms"
```

The rooms come back. `?immutable=1` tells SQLite the file won't change, so it
takes no locks, which a read-only folder can't hold.

**4. ATTACH a database outside the grant: refused**

`sqlite attach 'file:/workshop/private-data/payroll.db?immutable=1' as p; select name, salary from p.salaries`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d "sqlite attach 'file:/workshop/private-data/payroll.db?immutable=1' as p; select name, salary from p.salaries"
```

The same command with another path. `payroll.db` is a valid database on the
host pod's disk, but it is not in this component's filesystem, so SQLite
can't open it. The tool reports SQLite's "unable to open database" as
`access-denied`.

On your laptop, native sqlite3 attaches the same file (optional, needs
sqlite3):

```sh
sqlite3 databases/talks.db \
  "attach 'file:databases/payroll.db?immutable=1' as p; select name, salary from p.salaries"
```

**5. .shell has nothing to start:** `sqlite .shell id`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'sqlite .shell id'
```

The tool's own shell code answers this: "there is no shell here". That
message is the tool's choice. The reason it couldn't do otherwise is below.

**6. load_extension doesn't exist:** `sqlite select load_extension('/tmp/evil')`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d "sqlite select load_extension('/tmp/evil')"
```

"no such function": extension loading isn't compiled into this SQLite.

## The proof is the imports

The messages above come from the tool. What it can actually do comes from its
imports:

```sh
wasm-tools component wit artifacts/tool_sqlite.wasm | grep -E '^  import'
```

The list (17 lines) is clocks, streams, terminal, environment, exit and
filesystem. WASI has no interface that starts a process, and there is no HTTP
client or socket. Even a sqlite3 with a real `.shell` would have nothing to
start, and the filesystem holds only `/data`.

Full transcript: [`expected-output.txt`](expected-output.txt).
