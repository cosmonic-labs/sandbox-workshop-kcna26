# Hands-on test checklist

Run these before the workshop, on machines that have never seen this repo.
Each run should take 15 to 20 minutes. Note anything that is slow, unclear or
red, with the exact output.

## 1. Fresh clone on macOS or Linux, with Docker

Start Docker first (Docker Desktop, OrbStack or Podman, 4 GB or more for
containers).

```sh
git clone https://github.com/ericgregory/agent-toolbox-workshop
cd agent-toolbox-workshop
make doctor
```

- [ ] `make doctor` before `make tools`: red `FAIL` on wash, kind, kubectl and
      helm, each with `fix: make tools`. Exit is nonzero.
- [ ] `make tools`: eight `ok` lines (wash, nats-server, nats, zot,
      wasm-tools, kind, kubectl, helm), under a minute on good Wi-Fi.
- [ ] `make doctor`: every Required line green, ending in `Ready.`
- [ ] `make up`: ends with the cluster, registry and ingress lines and a
      `stores` line, in about 2 minutes the first time. `kubectl config
      current-context` says `kind-workshop-toolbox`.
- [ ] `export PATH="$PWD/.tools:$PATH"`, then walk steps 1 to 7 by hand from
      each `steps/*/README.md`. Every command runs as written and the output
      matches the **Expected** block.
- [ ] `make prove`: ends with `proofs written:` and seven files. `git diff
      steps` shows only host ids, the service account token prefix and pod
      names.
- [ ] `make clean`: the cluster and the registry container are gone; your
      previous kubectl context still exists (`kubectl config get-contexts`).

## 2. No Docker: the local fallback

On a machine with Docker stopped:

- [ ] `make doctor` fails the Docker line and shows the fallback section green.
- [ ] `make up-local && make push-local`, then steps 1 to 3 and 5 to 7 with
      `python3 scripts/apply.py <manifest>` instead of `kubectl apply -f`.
- [ ] `make prove-local`: six files in `.run/local/`.

## 3. GitHub Codespaces (not yet verified)

- [ ] Create a codespace on a 4 core machine. The build finishes and runs
      `make doctor` on its own; every Required line is green (Docker comes
      from the docker-in-docker feature).
- [ ] `make up` completes inside the codespace. Note how long it takes.
- [ ] Steps 1, 3, 4 and 5 run as written from the codespace terminal.

## 4. Build it yourself

- [ ] With Rust 1.98.1 and the `wasm32-wasip2` target: `make build && make
      push-build`, then step 5 still refuses the leak.

## 5. In the room

- [ ] Conference Wi-Fi: `make up` on a machine that ran `make tools && make
      up` the day before needs no downloads (kind's node image and the chart
      images are cached).
- [ ] Jumping ahead works: on a fresh `make up`, `kubectl apply -f
      steps/05-web/workload.yaml` alone, then step 5's commands.
