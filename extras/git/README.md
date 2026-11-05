# Extra: Read a hostile repository with git

**Folder:** `extras/git`

Extra credit: the session doesn't cover this one. It builds on step 3's
cluster state. Its manifest is `steps/03-sqlite/workload.yaml` with one
change: the `git` slot, which holds the placeholder in every step, gets the
real git tool and the repository, read only. The repository is already on
your cluster: `make up` builds it into `.run/repo` for this extra.

`git` is on [GTFOBins](https://gtfobins.github.io/) because a repository can
tell git to run a program: a hook, `core.fsmonitor`, `core.pager`, a bang
alias, a smudge or textconv filter, a credential helper, an external diff.
The repository mounted here (`fixtures/make-hostile-repo.sh`) sets every one
of those to a marker command, `touch /tmp/gtfobins-git-<name>`. `touch` makes
an empty file, so each file in `/tmp` named `gtfobins-git-...` shows that git
ran that entry.

## First, watch native git run them

On your own machine, with your own git, against the same repository:

```sh
git -C .run/repo status
ls /tmp/gtfobins-git-*
```

```
warning: Empty last update token.
fatal: the remote end hung up unexpectedly
/tmp/gtfobins-git-filterprocess
/tmp/gtfobins-git-fsmonitor
```

Just asking for the status ran two programs the repository named:
`core.fsmonitor` and the filter's `process` command. The two error lines come
after they ran: git expected each program to answer in its own protocol, and
`touch` doesn't. A diff runs more (optional):

```sh
git -C .run/repo diff HEAD~2 HEAD
ls /tmp/gtfobins-git-*
```

This adds `/tmp/gtfobins-git-diffexternal`, from `diff.external`. The
markers are empty files and harmless. Remove them:

```sh
rm -f /tmp/gtfobins-git-*
```

## What the git tool is

The git tool is a reduced, read-only git interface, built on
[gitoxide](https://github.com/GitoxideLabs/gitoxide) (a git written in Rust).
It offers `log`, `status`, `blame`, `show`, `diff`, `ls-files`, `ls-tree` and
`cat-file`, and nothing that writes. Some of these are smaller than native
git's: `status` reports only the branch and the tip, and `show` and `diff`
list changed file names, not a patch. So those commands never reach the code
where native git starts `core.fsmonitor` or a textconv filter. They are not
evidence on their own.

The real point is that the repository's config names programs, and this
component has no way to start a program. Its imports prove it:

```sh
wasm-tools component wit artifacts/tool_git.wasm | grep -E '^  import'
```

```
  import wasi:io/poll@0.2.9;
  import wasi:clocks/monotonic-clock@0.2.9;
  import wasi:cli/stdout@0.2.9;
  import wasi:cli/environment@0.2.9;
  import wasi:filesystem/types@0.2.9;
  import wasi:filesystem/preopens@0.2.9;
  import wasi:random/insecure-seed@0.2.9;
  ...
```

(Abbreviated: the full list has 18 lines, all clocks, streams, terminal,
environment, exit, filesystem and random.) WASI, the set of interfaces a
component can import from the host, has no interface that starts a process.
A component can't import what doesn't exist, so nothing the repository
configures can run, whatever gitoxide's code does with it. There is no HTTP
client and no socket either, so nothing can reach the network.

## Deploy

```sh
kubectl apply -f extras/git/workload.yaml \
  && sleep 2 && kubectl wait --for=condition=Ready \
  workloaddeployment/toolbox-agent
```

The same agent as step 3, with the real git tool in its slot. Its grant: the
repository, read only.

```yaml
volumeMounts: [{ name: repo, mountPath: /repo, readOnly: true }]
```

## Try each command

Type each prompt into the chat at http://agent.localhost:18180, or run the
curl command under it.

**1. log reads commits:** `git log --oneline -5`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'git log --oneline -5'
```

Real commits come back, read from the repository's objects.

**2. status, the command that ran two programs natively:** `git status`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'git status'
```

`On branch main HEAD is 72e53e6`. This status reports only the branch and the
tip, so it never looks at `core.fsmonitor`. Even if it did, the component
could not start the program.

**3. blame reads line history:** `git blame README.md`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'git blame README.md'
```

Line by line history, straight from the repository's objects.

**4. push is refused:** `git push origin main`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'git push origin main'
```

The tool's own code refuses this: it offers no command that writes or reaches
the network. That is the tool's choice, not the sandbox. The sandbox is the
second wall: the component has no network import and its folder is mounted
read only.

Full transcript: [`expected-output.txt`](expected-output.txt).
