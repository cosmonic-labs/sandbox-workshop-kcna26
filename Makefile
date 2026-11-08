# Shortcuts for the workshop. Each target runs a script in scripts/, which
# stays the source of truth: read the script to see exactly what happens.
#
#   make tools       download the pinned tools into .tools/ (checksums verified)
#   make doctor      check this machine; changes nothing
#   make up          kind cluster + the wasmCloud chart; pushes artifacts/; maps :28180 to the ingress NodePort
#   make push-build  push your curl build to the cluster's registry as tool-curl:mine (step 2)
#   make build       build the curl tool (needs Rust 1.98.1 + wasm32-wasip2)
#   make build-all   build every component, including sqlite (needs wasi-sdk)
#   make artifacts   rebuild artifacts/ from source in the build image (maintainers; needs Docker)
#   make check-artifacts  artifacts/ matches a build of this source (CI runs this)
#   make guide       rebuild artifacts/ (guide.wasm included) after editing a README; needs Docker
#   make databases rebuild databases/*.db from their .sql (maintainers; needs sqlite3)
#   make check-databases  the committed .db files match their checksums and sources
#   make prove       replay steps 1 to 4 on the cluster, rewrite their expected-output.txt
#   make verify      replay the same, change nothing, check each result is what it should be
#   make github-sign-in   sign step 4's agent in to GitHub from the terminal (code flow, for Codespaces)
#   make github-sign-out  sign it out again
#   make record      re-record docs/walkthrough.cast from the READMEs (needs asciinema)
#   make down        changes nothing; make clean deletes the cluster and the registry
#
#   make desktop-up     run the workshop on a development Cosmonic Desktop daemon (DESKTOP.md)
#   make desktop-step STEP=2  switch Desktop to step 1, 2, 3, or 4
#   make desktop-verify  replay and check the Desktop demo; LIVE=1 includes a GitHub read
#   make desktop-down   stop only the workshop Desktop daemon
#
#   No Docker? Open the repo in GitHub Codespaces (see README.md).

SHELL := /bin/bash
.DEFAULT_GOAL := help

.PHONY: help tools doctor up push-build build build-all artifacts check-artifacts guide databases check-databases prove verify github-sign-in github-sign-out record down clean

help:
	@sed -n 's/^#   //p' $(firstword $(MAKEFILE_LIST))

tools:
	@./scripts/tools.sh

doctor:
	@./scripts/doctor.sh

up:
	@./scripts/up.sh

push-build:
	@source scripts/env.sh && REGISTRY="localhost:$$KIND_REGISTRY_PORT" ./scripts/push.sh --mine

# Only curl, the tool step 2 asks you to personalize. It is pure Rust, so it
# needs no C toolchain. sqlite ships prebuilt in artifacts/ because
# rebuilding it needs wasi-sdk; use build-all for that.
build:
	@./scripts/build.sh -p tool-curl

# Every component. sqlite's bundled SQLite is C, so this needs wasi-sdk: set
# WASI_SDK_PATH (scripts/build.sh).
build-all:
	@./scripts/build.sh

# artifacts/ is built from this source in the pinned build image
# (build.Dockerfile). CI rebuilds it on every push and pull request, fails
# when it differs, and publishes its build to the repo's packages from main.
artifacts:
	@./scripts/artifacts.sh update

check-artifacts:
	@./scripts/artifacts.sh check

# The guide (http://guide.localhost:28180) is the READMEs rendered at build
# time, so it is rebuilt whenever one changes; make verify says when it is
# stale. Built like the rest of artifacts/, in the build image, so CI's check
# agrees. make up deploys the new build.
guide: artifacts

databases:
	@./scripts/databases.sh build

check-databases:
	@./scripts/databases.sh check

prove:
	@./scripts/prove.sh

verify:
	@./scripts/prove.sh --verify

github-sign-in:
	@./scripts/github-sign-in.sh

github-sign-out:
	@./scripts/github-sign-in.sh --out

record:
	@./scripts/record.sh

down:
	@./scripts/down.sh

clean:
	@./scripts/down.sh --all

# Desktop uses the committed components with a local development daemon.
# See DESKTOP.md. up runs in the foreground; use another terminal for the steps.
.PHONY: desktop-up desktop-step desktop-verify desktop-down

node_modules/yaml/package.json: package.json package-lock.json
	@npm ci --ignore-scripts --no-audit --no-fund

desktop-up desktop-step desktop-verify desktop-down: node_modules/yaml/package.json

desktop-up:
	@node scripts/desktop.mjs up

desktop-step:
	@node scripts/desktop.mjs step $(STEP)

desktop-verify:
	@node scripts/desktop.mjs verify $(if $(LIVE),--live)

desktop-down:
	@node scripts/desktop.mjs down
