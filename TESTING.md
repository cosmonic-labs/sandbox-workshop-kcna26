# Hands-on test checklist

Run these before the workshop, on machines that have never seen this repo.
Each run should take 15 to 20 minutes. Note anything that is slow, unclear or
red, with the exact output.

## 1. Fresh clone on macOS or Linux, with Docker

Start Docker first (Docker Desktop, OrbStack or Podman, 4 GB or more for
containers).

```sh
git clone https://github.com/cosmonic-labs/sandbox-workshop-kcna26
cd sandbox-workshop-kcna26
make doctor
```

- [ ] `make doctor` before `make tools`: red `FAIL` on wash, kind, kubectl,
      helm and nats, each with `fix: make tools`. Exit is nonzero.
- [ ] `make tools`: six `ok` lines (wash, nats, wasm-tools, kind, kubectl,
      helm), under a minute on good Wi-Fi.
- [ ] `make doctor`: every Required line green, ending in `Ready.`
- [ ] `make up`: copies the prebuilt databases, then ends with the cluster,
      registry and ingress lines and a `stores` line, in about 2 minutes the
      first time.
      `kubectl config current-context` says `kind-workshop-toolbox`.
- [ ] `make check-fixtures-db` (maintainers, needs sqlite3): ends with
      `fixtures/talks.db: matches fixtures/talks.sql`.
- [ ] `export PATH="$PWD/.tools:$PATH"`, then walk steps 1 to 4 by hand from
      each `steps/*/README.md`. Every command runs as written and the output
      matches the result line under it.
- [ ] `make prove`: ends with `proofs written:` and the step files.
      `git diff steps` shows nothing: the transcripts are deterministic
      (step 4's live GitHub answers are written as
      `<live data from GitHub>`).
- [ ] `make verify`: replays steps 1 to 4 without writing anything and ends
      with `verify: all N checks passed` (or, when GitHub's unauthenticated
      limit is used up, `N live GitHub checks skipped`). It checks each
      result, not the exact text: allowed calls return what they should, each
      refusal has the expected error and the refusing layer's message (the
      grant, the tool's own code, or the github-api plugin), and step 2's
      control reverts. A mismatch prints `FAIL`, what was wanted and what
      came back, and exits nonzero.
- [ ] Every `$ curl ...` line in `steps/*/expected-output.txt` pastes into a
      shell and runs as written.
- [ ] `make clean`: the cluster and the registry container are gone; your
      previous kubectl context still exists (`kubectl config get-contexts`).

## 2. GitHub Codespaces, the no-Docker path

- [ ] Create a codespace on a 4 core machine. The build finishes and runs
      `make doctor` on its own; every Required line is green (Docker comes
      from the docker-in-docker feature).
- [ ] `make up` completes inside the codespace. Note how long it takes.
- [ ] Steps 1 to 4 run as written from the codespace terminal (step 2 with the prebuilt curl, step 4's
      sign-in with the curl commands its README gives).

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
      steps/04-github/workload.yaml` alone, then step 4's commands.
- [ ] Conference Wi-Fi and GitHub: everyone in the room shares one IP
      address, so the unauthenticated limit (60 requests an hour) runs out
      fast. Signed in, each person gets their own 5,000.

## 5. GitHub sign-in (once the OAuth app's client id is in k8s/values.yaml)

- [ ] Step 4's chat page: **Sign in with GitHub** shows a code and a link;
      entering the code on github.com turns it into `Signed in to GitHub as
      <you>` within a few seconds, and `gh` answers end with
      `(signed in)` and a limit of 5000.
- [ ] `gh api -X POST /repos/wasmCloud/wasmCloud/issues/5885/comments`,
      signed in: still `github-api plugin: only GET requests are allowed`.
- [ ] **Sign out** returns to `Sign in with GitHub`; applying another
      manifest or restarting the host pod signs you out too.
