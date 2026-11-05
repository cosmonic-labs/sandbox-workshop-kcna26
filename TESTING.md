# Hands-on test checklist

Run these before the workshop, on machines that have never seen this repo.
Each run should take 15 to 20 minutes. Note anything that is slow, unclear or
red, with the exact output.

## 1. Fresh clone on macOS or Linux, with Docker

Start Docker first (Docker Desktop, OrbStack or Podman, 4 GB or more for
containers).

```sh
git clone https://github.com/cosmonic-labs/agent-tools-workshop
cd agent-tools-workshop
make doctor
```

- [ ] `make doctor` before `make tools`: red `FAIL` on wash, kind, kubectl,
      helm and nats, each with `fix: make tools`. Exit is nonzero.
- [ ] `make tools`: six `ok` lines (wash, nats, wasm-tools, kind, kubectl,
      helm), under a minute on good Wi-Fi.
- [ ] `make doctor`: every Required line green, ending in `Ready.`
- [ ] `make up`: copies the prebuilt databases and builds the hostile
      repository (for `extras/git`), then ends with the cluster, registry and ingress lines and
      a `stores` line, in about 2 minutes the first time.
      `kubectl config current-context` says `kind-workshop-toolbox`.
- [ ] `make check-fixtures-db` (maintainers, needs sqlite3): ends with
      `fixtures/talks.db: matches fixtures/talks.sql`.
- [ ] `export PATH="$PWD/.tools:$PATH"`, then walk steps 1 to 3 by hand from
      each `steps/*/README.md`, then `extras/kubectl/README.md` and
      `extras/git/README.md`. Every command
      runs as written and the output matches the result line under it.
- [ ] `make prove`: ends with `proofs written:` and the step and extra
      files. `git diff steps extras` shows nothing: the transcripts are
      deterministic (pod names are written as `hello-<pod>` and kubectl's
      ages as `<age>`; the git extra's native control in
      `extras/git/control-checks.txt` depends on the local git version).
      `extras/real-model` is not replayed: it needs a real model.
- [ ] `make verify`: replays steps 1 to 3, `extras/kubectl` and
      `extras/git` without writing anything and ends with
      `verify: all N checks passed`. It checks each result, not the
      exact text: allowed calls return what they should, each refusal has the
      expected error and the refusing layer's message (the grant, the tool's
      own code, or RBAC), the native git control runs the repository's
      programs while the git tool leaves no marker on the host pod, and both
      controls revert. A mismatch prints `FAIL`, what was wanted and what came
      back, and exits nonzero.
- [ ] Every `$ curl ...` line in `steps/*/expected-output.txt` and
      `extras/*/expected-output.txt` pastes into a shell and runs as
      written.
- [ ] `make clean`: the cluster and the registry container are gone; your
      previous kubectl context still exists (`kubectl config get-contexts`).

## 2. GitHub Codespaces, the no-Docker path

- [ ] Create a codespace on a 4 core machine. The build finishes and runs
      `make doctor` on its own; every Required line is green (Docker comes
      from the docker-in-docker feature).
- [ ] `make up` completes inside the codespace. Note how long it takes.
- [ ] Steps 1 to 3, `extras/kubectl` and `extras/git` run as written from
      the codespace terminal (step 2 with the prebuilt curl).

## 3. Build curl yourself

- [ ] With Rust 1.98.1 and the `wasm32-wasip2` target, follow step 2's
      "Build your own" section and each "Your build" command: change the
      save message, `make build && make push-build` (prints
      `pushed localhost:15052/workshop/tool-curl:mine`), apply
      `steps/02-curl/workload-mine.yaml`, and confirm the save prompt
      answers with your message.

## 4. In the room

- [ ] Conference Wi-Fi: `make up` on a machine that ran `make tools && make
      up` the day before needs no downloads (kind's node image and the chart
      images are cached).
- [ ] Jumping ahead works: on a fresh `make up`, `kubectl apply -f
      extras/kubectl/workload.yaml` alone, then that extra's commands.

## 5. Extra: a real model

- [ ] With an API key, or a local model server such as Ollama with
      `qwen3:4b`, follow `extras/real-model/README.md`. Both manifests come
      up Ready; the model calls a tool and answers from its result, and the
      metadata prompt shows the same `access-denied` line as step 2. The
      wording differs from run to run.
- [ ] The offline check in that README (the fake endpoint) answers with
      `(fake-llm) the tool said: ...`.
