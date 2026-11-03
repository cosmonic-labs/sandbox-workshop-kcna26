# Step 5: GTFOBins, git

**Folder:** `steps/05-git`

`git` is on [GTFOBins](https://gtfobins.github.io/) because a repository can
tell git to run a program: a hook, `core.fsmonitor`, `core.pager`, a bang
alias, a smudge or textconv filter, a credential helper. Native git runs
several of them just to read or diff a repository. The repository mounted here
(`fixtures/make-hostile-repo.sh`) wires every one of those to a marker command.

This git tool is built on [gitoxide](https://github.com/GitoxideLabs/gitoxide),
a pure Rust git that runs none of those commands on its own, and it is a
component that can start no process at all. So the config does nothing. Only
commands that read are offered; anything that writes or reaches the network is
refused.

**Run:**

```sh
kubectl apply -f steps/05-git/workload.yaml && sleep 2 && kubectl wait --for=condition=Ready workloaddeployment/toolbox-agent

curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d 'git log --oneline -5'
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d 'git show HEAD'
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d 'git cat-file -p HEAD:README.md'
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d 'git blame README.md'
# A command that writes or reaches the network: refused
curl -s -H 'Host: agent.localhost' http://127.0.0.1:18180/ -d 'git push origin main'
```

**Expected:** `log`, `show`, `cat-file` and `blame` all return real data. None
of the configured commands runs: reading a tracked file does not invoke the
smudge filter, and a diff does not invoke textconv. `push` is refused because
this git reads only.

Full transcript: `expected-output.txt`.
