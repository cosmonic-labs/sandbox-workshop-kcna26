# Shortcuts for the workshop. Each target runs a script in scripts/, which
# stays the source of truth: read the script to see exactly what happens.
#
#   make tools       download the pinned tools into .tools/ (checksums verified)
#   make doctor      check this machine; changes nothing
#   make up          kind cluster + the wasmCloud chart; pushes artifacts/; maps :18180 to the ingress NodePort
#   make push-build  push your curl build to the cluster's registry as tool-curl:mine (step 2)
#   make build       build the curl tool (needs Rust 1.98.1 + wasm32-wasip2)
#   make build-all   build every component, including sqlite (needs wasi-sdk)
#   make fixtures-db rebuild fixtures/*.db from their .sql (maintainers; needs sqlite3)
#   make check-fixtures-db  the committed .db files match their checksums and sources
#   make prove       replay steps 1 to 4 on the cluster, rewrite their expected-output.txt
#   make verify      replay the same, change nothing, check each result is what it should be
#   make github-sign-in   sign step 4's agent in to GitHub from the terminal (code flow, for Codespaces)
#   make github-sign-out  sign it out again
#   make down        changes nothing; make clean deletes the cluster and the registry
#
#   No Docker? Open the repo in GitHub Codespaces (see README.md).

SHELL := /bin/bash
.DEFAULT_GOAL := help

.PHONY: help tools doctor up push-build build build-all fixtures-db check-fixtures-db prove verify github-sign-in github-sign-out down clean

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
	CARGO_BUILD_JOBS=$${CARGO_BUILD_JOBS:-4} cargo build --release --target wasm32-wasip2 -p tool-curl

# Every component, for maintainers who regenerate artifacts/. sqlite's bundled
# SQLite is C, so this needs wasi-sdk: set WASI_SDK_PATH and CC_wasm32_wasip2.
build-all:
	CARGO_BUILD_JOBS=$${CARGO_BUILD_JOBS:-4} cargo build --release --target wasm32-wasip2

fixtures-db:
	@./scripts/fixtures-db.sh build

check-fixtures-db:
	@./scripts/fixtures-db.sh check

prove:
	@./scripts/prove.sh

verify:
	@./scripts/prove.sh --verify

github-sign-in:
	@./scripts/github-sign-in.sh

github-sign-out:
	@./scripts/github-sign-in.sh --out

down:
	@./scripts/down.sh

clean:
	@./scripts/down.sh --all
