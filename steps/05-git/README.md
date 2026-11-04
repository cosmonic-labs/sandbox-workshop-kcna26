# Step 5: Now read a hostile repository

**Folder:** `steps/05-git`

`git` is on [GTFOBins](https://gtfobins.github.io/) because a repository can
tell git to run a program: a hook, `core.fsmonitor`, `core.pager`, a bang
alias, a smudge or textconv filter, a credential helper. Native git runs
several of them just to read or diff a repository. The repository mounted here
(`fixtures/make-hostile-repo.sh`) wires every one of those to a marker command.

This git tool is built on [gitoxide](https://github.com/GitoxideLabs/gitoxide),
a pure Rust git that runs none of those commands on its own, in a component
that can start no process at all. Only commands that read are offered.

## Deploy

```sh
kubectl apply -f steps/05-git/workload.yaml \
  && sleep 2 && kubectl wait --for=condition=Ready \
  workloaddeployment/toolbox-agent
```

The grant: the repository, read only.

```yaml
volumeMounts: [{ name: repo, mountPath: /repo, readOnly: true }]
```

## Try each command

Type each prompt into the chat at http://agent.localhost:18180, or run the curl command under it.

**1. git log reads, nothing runs:** `git log --oneline -5`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'git log --oneline -5'
```

Real commits come back. The repository's commands have nothing to run in.

**2. git show, no pager, no textconv:** `git show HEAD`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'git show HEAD'
```

show prints a diff, where native git would run the pager and textconv. Neither runs here.

**3. Reading a file runs no filter:** `git cat-file -p HEAD:README.md`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'git cat-file -p HEAD:README.md'
```

The tracked file reads cleanly. The smudge filter, which native git runs on checkout, never runs.

**4. blame reads line history:** `git blame README.md`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'git blame README.md'
```

Line by line history, read straight from the repository's objects.

**5. push is refused:** `git push origin main`

```sh
curl -s -H 'Host: agent.localhost' \
  http://127.0.0.1:18180/ \
  -d 'git push origin main'
```

This git reads only. Anything that writes or reaches the network is refused.

Full transcript: [`expected-output.txt`](expected-output.txt).
