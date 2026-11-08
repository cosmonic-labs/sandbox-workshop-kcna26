# Run the workshop on Cosmonic Desktop

This path runs Desktop's development daemon and the workshop's chat page on
macOS or Linux. It needs Node.js 20 or newer and a development `cosmonicd`
built with [Desktop PR #853](https://github.com/cosmonic/desktop/pull/853).
Build it with `cargo build --locked --bin cosmonicd` from Desktop's `rust/`
directory. The released Desktop builds do not include this fix yet.

The fix lets the toolbox plugin use the agent SDK's shared tool records.
Previously Desktop dropped the plugin at startup because those types were
not linked. The workshop's tools and model run together in one workload.

## Start

From this repository, point to that built daemon and start the workshop:

```sh
export COSMONICD_BIN=/path/to/desktop/rust/target/debug/cosmonicd
make desktop-up
```

The helper installs its pinned Node dependency, checks the committed artifact
and database checksums, and starts a private workshop daemon. It installs the
two plugins through Desktop's API, restarts to load them, imports the local
components, and starts the mock sites and step 1. No registry push or cluster
is needed.

Wait for `Desktop workshop ready`, then open:

**http://agent.localhost:28190**

Keep that terminal open. Use another terminal in this repository for the
commands below. The terminal prints the state and log directory. Generated
Desktop manifests and verification results are written to `.run/desktop/`.

## Walk through the four steps

Switch to any step and wait for its `ready` line:

```sh
make desktop-step STEP=1
make desktop-step STEP=2
make desktop-step STEP=3
make desktop-step STEP=4
```

Run them one at a time as you present. Each step is complete, so jumping ahead
works. The helper waits for the new revision before announcing it is ready.

Use the chat page's example buttons or send prompts from the terminal:

```sh
node scripts/desktop.mjs ask 'hello'
node scripts/desktop.mjs ask 'fetch http://api.mock-sites.svc/data'
node scripts/desktop.mjs ask 'sqlite .tables'
node scripts/desktop.mjs ask 'gh pr list -R wasmCloud/wasmCloud'
```

Step 1 has no tools. Step 2 adds curl, step 3 adds SQLite, and step 4 adds gh.
Follow the explanations and prompts in the existing step READMEs, using
`make desktop-step` to deploy each step.

The two mock names are Kubernetes Service names. For Desktop, the helper gives
curl two address mappings in its own environment. Only those exact mock names
are mapped, before the host checks the destination against `allowedHosts`.
Two loopback-only bridges forward to the real mock-sites component:

| Mock site | Desktop address | Curl grant |
| --- | --- | --- |
| `api.mock-sites.svc` | `http://127.0.0.1:28191` | Allowed in steps 2–4 |
| `attacker.mock-sites.svc` | `http://127.0.0.1:28192` | Refused |

The browser's existing prompts therefore work. Explicit Desktop addresses work
too. A bridge fixes its upstream Host header, so a caller cannot use it to
select another workload. Each redirect is checked against the curl component's
grant. The agent and model receive no file or network grant.

SQLite gets copies of `talks.db` and `rooms.db` mounted read only at `/data`.
The helper also stages `payroll.db` outside that mount for the refused ATTACH
check. Curl gets a separate writable `/data` folder. The helper names the
installed toolbox and GitHub providers explicitly in the Desktop manifests.

Step 4 reads public GitHub pull requests while signed out. GitHub OAuth sign-in
is not configured by this helper. A live read uses GitHub's unauthenticated
rate limit, shared by everyone on the same public IP.

## Verify the demo

Replay every step and check its allowed calls and refusals:

```sh
make desktop-verify
```

Include one live GitHub read:

```sh
make desktop-verify LIVE=1
```

Verification checks the greeting, tool discovery, allowed HTTP, file saves and
reads, refused network and file access, redirects, SQLite queries and escapes,
and GitHub capability boundaries. It temporarily grants the attacker mock's
address to show it is reachable, then restores the original grant and verifies
that access is refused again. It finishes on step 4.

The helper's own tests run with:

```sh
npm ci --ignore-scripts
npm run test:desktop
```

## Stop or use different ports

Ctrl-C in the startup terminal, or:

```sh
make desktop-down
```

This stops the workshop daemon and bridges. The private state remains for the
next run. `make desktop-up` resets the workshop's database copies.

The ingress uses port 28190; the mock bridges use the next two ports. To choose
another set, export the same value in both terminals before running commands:

```sh
export DESKTOP_HTTP_PORT=28200
```

`DESKTOP_STATE_DIR` can choose a different private state directory. The helper
refuses to overwrite an existing directory that does not have this checkout's
workshop marker. It does not change system DNS or the machine signing policy;
plugin installation remains subject to Desktop's policy.
