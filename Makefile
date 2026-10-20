# Shortcuts for the workshop. Each target runs a script in scripts/, which
# stays the source of truth: read the script to see exactly what happens.
#
#   make tools      download the pinned tools into .tools/ (checksums verified)
#   make doctor     check this machine; changes nothing
#   make up         start NATS, the registry and the wasmCloud host
#   make push       push the prebuilt components in artifacts/ to the registry
#   make push-build push your own build from target/ instead
#   make build      build every component (needs Rust 1.98.1 + wasm32-wasip2)
#   make prove      replay every step and rewrite steps/*/expected-output.txt
#   make down       stop everything make up started (make clean also wipes data)
#   make kind-up    Kubernetes track: kind cluster + the upstream chart
#   make prove-k8s  Kubernetes track: replay the proofs on kind

SHELL := /bin/bash
.DEFAULT_GOAL := help

.PHONY: help tools doctor up push push-build build prove down clean kind-up prove-k8s

help:
	@sed -n 's/^#   //p' $(firstword $(MAKEFILE_LIST))

tools:
	@./scripts/tools.sh

doctor:
	@./scripts/doctor.sh

up:
	@./scripts/up.sh

push:
	@./scripts/push.sh artifacts

push-build:
	@./scripts/push.sh

build:
	CARGO_BUILD_JOBS=$${CARGO_BUILD_JOBS:-4} cargo build --release --target wasm32-wasip2

prove:
	@./scripts/prove.sh

down:
	@./scripts/down.sh

clean:
	@./scripts/down.sh --all

kind-up:
	@./scripts/kind-up.sh

prove-k8s:
	@./scripts/prove-k8s.sh
