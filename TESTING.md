# Hands-on test checklist

Run these before the workshop, on machines that have never seen this repo.
Each run should take 10 to 15 minutes. Note anything that is slow, unclear or
red, with the exact output.

## 1. Fresh clone on macOS

Use a Mac that has never run this repo (or a new user account). Docker can be
off; it is not needed.

```sh
git clone https://github.com/ericgregory/agent-toolbox-workshop
cd agent-toolbox-workshop
make doctor
```

- [ ] `make doctor` before `make tools`: red `FAIL` on wash and zot (and on
      nats-server and the nats CLI unless Homebrew already has them), each
      with `fix: make tools`. Exit is nonzero. No `.tools/` or `.run/`
      folder appeared.
- [ ] `make tools`: five `ok` lines (wash, nats-server, nats, zot,
      wasm-tools), about 230 MB, under a minute on good Wi-Fi.
- [ ] `make doctor`: every Required line green, ending in `Ready.` Yellow
      `--` lines are fine (Rust, kind and so on).
- [ ] `make up`: prints the nats, registry `(zot)` and host lines with a host id.
- [ ] `make push`: nine `pushed localhost:15051/workshop/...` lines.
- [ ] `make prove`: ends with `proofs written:` and seven files.
      `git diff steps` shows only the step 7 load test numbers.
- [ ] Walk steps 1 to 7 by hand from each `steps/*/README.md`. Every command
      prints what its **Expected** block shows.
- [ ] `make down`, then `make doctor`: ports show as free again.
- [ ] If you have Rust 1.98.1: `make build && make push-build`, then step 2
      still works.
- [ ] macOS on Intel, if you can borrow one: the same run.

## 2. Linux

Ubuntu 24.04 (x86_64 or arm64) in a VM or a spare machine, as a normal user.

- [ ] `make doctor` on a bare install names any missing system packages
      (`python3-yaml`, `netcat-openbsd`, `curl`) with the `sudo apt-get
      install` line to run. Run them, run `make doctor` again.
- [ ] Then the same list as macOS from `make tools` onward.
- [ ] `ab` missing is yellow only; `sudo apt-get install -y apache2-utils`
      makes it green (needed for `make prove`).

## 3. GitHub Codespaces

On the repo page: **Code > Codespaces > Create codespace on main**. (This
costs Codespaces minutes; delete the codespace afterwards.)

- [ ] The build finishes, and the creation log ends with `make doctor`
      output: all Required green, Rust 1.98.1 green, the Kubernetes line
      yellow. Note how long creation took.
- [ ] In the terminal: `make up && make push && make prove`. Same results
      as on macOS.
- [ ] The **Ports** tab lists 18180 as "wasmCloud host (agent and toy sites;
      use curl with a Host header)" and nothing popped open in the browser.
- [ ] Step 2's `nats ... kv ls` and step 1's `wasm-tools` work without
      changing PATH.
- [ ] `cargo build --release --target wasm32-wasip2 -p tool-memory` builds in
      seconds (crates were fetched at creation).
- [ ] Decide on prebuilds: without them every attendee waits for the image
      build and the Rust install. With prebuilds on, creation should take
      under a minute. The `INSTALL_RUST` build arg in
      `.devcontainer/devcontainer.json` can be set to `"false"` to save about
      1 GB.

## 4. Kubernetes track

On a machine with Docker running, plus kind, kubectl and helm.

- [ ] `make kind-up`: creates the `workshop-toolbox` cluster, starts the
      `workshop-toolbox-registry` container on `127.0.0.1:15052`, pushes nine
      components, installs the chart; the three pods end `Running`.
- [ ] `make prove-k8s`: `k8s/expected-output.txt` shows `READY True`, the
      same attack 1 and attack 2 transcripts as the local steps, `leaks=0`
      then `leaks=1` for the control, and three replicas after
      `kubectl scale`. `git diff k8s` shows only resource names, host ids,
      ages and `created` instead of `unchanged`.
- [ ] `make clean` removes the cluster and the registry container and leaves
      other clusters and containers alone.

## 5. Room conditions

- [ ] Run section 1 tethered to a phone, to feel conference Wi-Fi.
- [ ] Run `make doctor` on Windows without WSL (Git Bash, if `make` exists
      there): it should say to use WSL2 or GitHub Codespaces.
