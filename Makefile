# Shortcuts for the workshop. Each target runs a script in scripts/, which
# stays the source of truth: read the script to see exactly what happens.
#
#   make tools       download the pinned tools into .tools/ (checksums verified)
#   make doctor      check this machine; changes nothing
#   make up          kind cluster + the wasmCloud chart; pushes artifacts/; forwards :18180
#   make push-build  push your own build from target/ to the cluster's registry
#   make build       build every component (needs Rust 1.98.1 + wasm32-wasip2)
#   make prove       replay every step on the cluster, rewrite steps/*/expected-output.txt
#   make down        stop the port-forward (make clean also deletes the cluster)
#
#   No Docker? The same steps on a local wasmCloud host, applied with
#   scripts/apply.py instead of kubectl:
#   make up-local    start NATS, a registry and wash host; then make push-local
#   make push-local  push artifacts/ to the local registry
#   make prove-local replay every step on the local host

SHELL := /bin/bash
.DEFAULT_GOAL := help

.PHONY: help tools doctor up push-build build prove down clean up-local push-local prove-local

help:
	@sed -n 's/^#   //p' $(firstword $(MAKEFILE_LIST))

tools:
	@./scripts/tools.sh

doctor:
	@./scripts/doctor.sh

up:
	@./scripts/up.sh

push-build:
	@source scripts/env.sh && REGISTRY="localhost:$$KIND_REGISTRY_PORT" ./scripts/push.sh

build:
	CARGO_BUILD_JOBS=$${CARGO_BUILD_JOBS:-4} cargo build --release --target wasm32-wasip2

prove:
	@./scripts/prove.sh

down:
	@./scripts/down.sh

clean:
	@./scripts/down.sh --all

up-local:
	@./scripts/up-local.sh

push-local:
	@./scripts/push.sh artifacts

prove-local:
	@LOCAL=1 ./scripts/prove.sh
