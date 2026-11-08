#!/usr/bin/env bash
# Check that this machine is ready for the workshop. CHECK ONLY: this script
# never installs, downloads, starts or changes anything. Every problem it
# finds comes with the exact command that fixes it, for you to run.
#
#   OK    ready
#   FAIL  required: the workshop will not work until you fix it
#   --    optional: only needed for building tools yourself or make prove
#
# Exit status is nonzero when any required check fails.
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TOOLS="$ROOT/.tools"
WASH_VERSION=2.12.0
RUST_VERSION=1.98.1

if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
  G=$'\033[32m'; R=$'\033[31m'; Y=$'\033[33m'; B=$'\033[1m'; N=$'\033[0m'
else
  G=; R=; Y=; B=; N=
fi
FAILS=0
ok()   { printf '  %sOK  %s  %s\n' "$G" "$N" "$1"; }
fail() { printf '  %sFAIL%s  %s\n' "$R" "$N" "$1"; shift
         for l in "$@"; do printf '        %s\n' "$l"; done; FAILS=$((FAILS + 1)); }
opt()  { printf '  %s--  %s  %s\n' "$Y" "$N" "$1"; shift
         for l in "$@"; do printf '        %s\n' "$l"; done; }
section() { printf '\n%s%s%s\n' "$B" "$1" "$N"; }

# Find a tool in .tools/ first (where `make tools` puts it), then on PATH.
find_tool() {
  if [ -x "$TOOLS/$1" ]; then echo "$TOOLS/$1"; else command -v "$1" 2>/dev/null; fi
}

OS=$(uname -s); ARCH=$(uname -m)
case "$OS-$ARCH" in
  Darwin-arm64|Darwin-x86_64|Linux-x86_64|Linux-amd64|Linux-aarch64|Linux-arm64)
    PLATFORM_OK=1 ;;
  *) PLATFORM_OK=0 ;;
esac
pkg_hint() { # what to suggest for a system package, by OS
  if [ "$OS" = Darwin ]; then echo "$1"; else echo "$2"; fi
}

printf '%sWorkshop doctor%s: checks only, changes nothing.\n' "$B" "$N"

section "Your computer"
if [ "$PLATFORM_OK" = 1 ]; then
  ok "$OS $ARCH is supported"
else
  fail "$OS $ARCH is not supported directly" \
    "Use WSL2 (Windows Subsystem for Linux) or GitHub Codespaces instead:" \
    "on the repository page, choose Code > Codespaces > Create codespace on main."
  echo
  echo "${R}Not ready.${N} Use WSL2 or GitHub Codespaces."
  exit 1
fi

section "Required"

# wash: the wasmCloud command line tool. make up uses it to push the
# components to the cluster's registry.
if [ -x "$TOOLS/wash" ]; then
  v=$("$TOOLS/wash" --version 2>/dev/null | head -1)
  if [ "$v" = "wash $WASH_VERSION" ]; then
    ok "wash $WASH_VERSION in .tools/wash (the wasmCloud CLI; pushes the components)"
  else
    fail "wash in .tools/wash is '${v:-unreadable}', the workshop needs $WASH_VERSION" \
      "fix: ./scripts/tools.sh --force    (re-downloads every pinned tool)"
  fi
else
  fail "wash $WASH_VERSION is missing from .tools/wash (the wasmCloud CLI; pushes the components)" \
    "fix: make tools"
fi

# Docker, kind, kubectl and helm: the workshop runs on a kind cluster.
if ! command -v docker >/dev/null 2>&1; then
  fail "Docker is not installed (kind runs the Kubernetes cluster in Docker)" \
    "fix: install Docker Desktop, OrbStack or Podman (https://docs.docker.com/get-docker/)," \
    "     or use GitHub Codespaces (Code > Codespaces > Create codespace on main)"
elif ! docker info >/dev/null 2>&1; then
  fail "Docker is installed but not running" \
    "fix: $(pkg_hint 'open -a Docker' 'sudo systemctl start docker')    (then wait until it says it is running)"
else
  mem=$(docker info --format '{{.MemTotal}}' 2>/dev/null || echo 0)
  if [ "${mem:-0}" -lt 3500000000 ]; then
    fail "Docker has $((mem / 1000000)) MB of memory; the cluster needs about 4 GB" \
      "fix: raise Docker's memory limit to 4 GB or more (Docker Desktop: Settings > Resources)"
  else
    ok "Docker is running ($((mem / 1000000000)) GB for containers)"
  fi
fi
for t in kind kubectl helm; do
  p=$(find_tool "$t")
  if [ -n "$p" ]; then ok "$t ($p)"; else fail "$t is missing" "fix: make tools"; fi
done

# curl and nc: the scripts and every step use them.
for t in curl nc; do
  if command -v "$t" >/dev/null 2>&1; then
    ok "$t"
  else
    fail "$t is missing (the scripts use it)" \
      "fix: $(pkg_hint "brew install $t" "sudo apt-get install -y $( [ "$t" = nc ] && echo netcat-openbsd || echo curl )")"
  fi
done

# The ingress port. The workshop's own kind cluster holding it counts as fine.
port="${HOST_HTTP:-127.0.0.1:28180}"; port="${port##*:}"
if ! (command -v nc >/dev/null 2>&1 && nc -z 127.0.0.1 "$port" 2>/dev/null); then
  ok "port $port is free"
elif command -v docker >/dev/null 2>&1 &&
     docker ps --format '{{.Names}}' 2>/dev/null | grep -qx workshop-toolbox-control-plane; then
  ok "port $port is in use by this workshop's cluster (make clean deletes it)"
  # The cluster is up, so the guide should answer (k8s/guide.yaml).
  if [ "$(curl -s -o /dev/null -m 5 -w '%{http_code}' -H 'Host: guide.localhost' "http://127.0.0.1:$port/")" = 200 ]; then
    ok "the guide answers at http://guide.localhost:$port"
  else
    opt "the guide does not answer at http://guide.localhost:$port" "fix: make up    (deploys k8s/guide.yaml)"
  fi
else
  fail "port $port is taken by another program" \
    "see which one: lsof -nP -iTCP:$port -sTCP:LISTEN" \
    "fix: stop that program, or pick another port: HOST_HTTP=127.0.0.1:28190 make up"
fi

if command -v sha256sum >/dev/null 2>&1; then sumcmd="sha256sum -c"; else sumcmd="shasum -a 256 -c"; fi

# Prebuilt components.
if [ -f "$ROOT/artifacts/SHA256SUMS" ]; then
  # SHA256SUMS lists repo-relative paths (artifacts/<name>.wasm).
  if (cd "$ROOT" && $sumcmd artifacts/SHA256SUMS >/dev/null 2>&1); then
    ok "prebuilt components in artifacts/ match SHA256SUMS"
  else
    bad=$(cd "$ROOT" && $sumcmd artifacts/SHA256SUMS 2>/dev/null | grep -v ': OK$' | cut -d: -f1 | tr '\n' ' ')
    fail "prebuilt components in artifacts/ do not match SHA256SUMS: ${bad:-unknown}" \
      "fix: git checkout -- artifacts    (restores the committed files)"
  fi
else
  fail "artifacts/SHA256SUMS is missing" "fix: git checkout -- artifacts"
fi

# The guide's pages are the READMEs as they were when artifacts/guide.wasm was
# built. It carries the cksum of each README (served at /sources), so a
# README edited since shows up here.
if [ -f "$ROOT/artifacts/guide.wasm" ]; then
  stale=$(cd "$ROOT" && cksum README.md steps/*/README.md mock-sites/README.md | while read -r line; do
    grep -qaF "$line" artifacts/guide.wasm || printf '%s ' "${line##* }"
  done)
  if [ -z "$stale" ]; then
    ok "the guide in artifacts/guide.wasm matches the READMEs"
  else
    opt "the guide in artifacts/guide.wasm is older than: $stale" \
      "fix: make guide && make up    (maintainers; needs Rust)"
  fi
fi

# Prebuilt databases (scripts/stage-data.sh copies them into .run for kind).
if (cd "$ROOT" && $sumcmd databases/SHA256SUMS >/dev/null 2>&1); then
  ok "prebuilt databases in databases/ match SHA256SUMS"
else
  fail "prebuilt databases in databases/ do not match databases/SHA256SUMS" \
    "fix: git checkout -- databases    (restores the committed files)"
fi

section "Optional"

p=$(find_tool wasm-tools)
if [ -n "$p" ]; then
  ok "wasm-tools $("$p" --version 2>/dev/null | awk '{print $2}') (lists a component's imports)"
else
  opt "wasm-tools is missing: needed for make prove and step 1's 'look inside'" "fix: make tools"
fi

# sqlite3, only for maintainers who rebuild databases/*.db (make databases).
if command -v sqlite3 >/dev/null 2>&1; then
  ok "sqlite3 (only for make databases; the workshop uses the prebuilt databases/*.db)"
else
  opt "sqlite3 is missing: only needed for make databases; the workshop uses the prebuilt databases/*.db" \
    "fix: $(pkg_hint 'brew install sqlite' 'sudo apt-get install -y sqlite3')"
fi

# Rust, only to build the components yourself. RUSTUP_AUTO_INSTALL=0 stops
# rustup from installing the pinned toolchain just because we asked.
if command -v rustup >/dev/null 2>&1; then
  if rustup toolchain list 2>/dev/null | grep -q "^$RUST_VERSION"; then
    if RUSTUP_AUTO_INSTALL=0 rustup target list --installed --toolchain "$RUST_VERSION" 2>/dev/null \
         | grep -qx wasm32-wasip2; then
      ok "Rust $RUST_VERSION with wasm32-wasip2 (build the tools yourself)"
    else
      opt "Rust $RUST_VERSION is installed without the wasm32-wasip2 target (only needed to build)" \
        "fix: rustup target add wasm32-wasip2 --toolchain $RUST_VERSION"
    fi
  else
    opt "Rust $RUST_VERSION is not installed (only needed to build; the prebuilt artifacts/ work without it)" \
      "fix: rustup toolchain install $RUST_VERSION --target wasm32-wasip2"
  fi
elif [ -x "${CARGO_HOME:-$HOME/.cargo}/bin/rustup" ]; then
  opt "Rust is installed in ${CARGO_HOME:-$HOME/.cargo}/bin, but that folder is not on your PATH (only needed to build)" \
    "fix: source \"${CARGO_HOME:-$HOME/.cargo}/env\"    (add that line to your shell profile to keep it)"
else
  opt "Rust (rustup) was not found on your PATH (only needed to build; the prebuilt artifacts/ work without it)" \
    "fix: see https://rustup.rs, then: rustup toolchain install $RUST_VERSION --target wasm32-wasip2"
fi

echo
if [ "$FAILS" -gt 0 ]; then
  echo "${R}Not ready:${N} $FAILS required check(s) failed. Run each 'fix:' command above, then make doctor again."
  exit 1
fi
echo "${G}Ready.${N} Next: make up, then follow steps/01-agent/README.md."
