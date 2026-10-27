#!/usr/bin/env bash
# Check that this machine is ready for the workshop. CHECK ONLY: this script
# never installs, downloads, starts or changes anything. Every problem it
# finds comes with the exact command that fixes it, for you to run.
#
#   OK    ready
#   FAIL  required: the workshop will not work until you fix it
#   --    optional: only needed for building tools yourself, make prove, or
#         the no-Docker fallback (make up-local)
#
# Exit status is nonzero when any required check fails.
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TOOLS="$ROOT/.tools"
RUN_DIR="$ROOT/.run"
REGISTRY_MODE="${REGISTRY_MODE:-zot}"
WASH_VERSION=2.11.0
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

# wash: the wasmCloud command line tool. Its `host` subcommand is the runtime.
if [ -x "$TOOLS/wash" ]; then
  v=$("$TOOLS/wash" --version 2>/dev/null | head -1)
  if [ "$v" = "wash $WASH_VERSION" ]; then
    ok "wash $WASH_VERSION in .tools/wash (the wasmCloud CLI and host)"
  else
    fail "wash in .tools/wash is '${v:-unreadable}', the workshop needs $WASH_VERSION" \
      "fix: ./scripts/tools.sh --force    (re-downloads every pinned tool)"
  fi
else
  fail "wash $WASH_VERSION is missing from .tools/wash (the wasmCloud CLI and host)" \
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

# Ports. One that this workshop's own stack holds (make up) counts as fine.
own_pid_for_port() {
  case "$1" in
    14222) echo nats ;; 15051) echo registry ;; 18180) echo pf ;; 18181) echo host ;;
  esac
}
for port in 18180; do
  if ! (command -v nc >/dev/null 2>&1 && nc -z 127.0.0.1 "$port" 2>/dev/null); then
    ok "port $port is free"
    continue
  fi
  f="$RUN_DIR/$(own_pid_for_port "$port").pid"
  if [ -f "$f" ] && kill -0 "$(cat "$f")" 2>/dev/null; then
    ok "port $port is in use by this workshop (make up is running; make down stops it)"
  elif [ "$port" = 15051 ] && command -v docker >/dev/null 2>&1 &&
       docker ps --format '{{.Names}}' 2>/dev/null | grep -qx workshop-toolbox-registry; then
    ok "port $port is in use by this workshop's registry container"
  else
    case "$port" in
      18180) alt="or pick another port for this run: HOST_HTTP=127.0.0.1:18190 make up" ;;
      18181) alt="or pick another port for this run: HOST_PROBE=127.0.0.1:18191 make up" ;;
      *) alt="(this port is fixed in the workshop files, so the other program has to stop)" ;;
    esac
    fail "port $port is taken by another program" \
      "see which one: lsof -nP -iTCP:$port -sTCP:LISTEN" \
      "fix: stop that program, $alt"
  fi
done

# Prebuilt components.
if [ -f "$ROOT/artifacts/SHA256SUMS" ]; then
  if command -v sha256sum >/dev/null 2>&1; then sumcmd="sha256sum -c"; else sumcmd="shasum -a 256 -c"; fi
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

section "Optional"

p=$(find_tool wasm-tools)
if [ -n "$p" ]; then
  ok "wasm-tools $("$p" --version 2>/dev/null | awk '{print $2}') (lists a component's imports)"
else
  opt "wasm-tools is missing: needed for make prove and step 1's 'look inside'" "fix: make tools"
fi

if command -v ab >/dev/null 2>&1; then
  ok "ab (load test in make prove, step 7)"
else
  opt "ab (ApacheBench) is missing: needed only for make prove's load test in step 7" \
    "fix: $(pkg_hint 'it ships with macOS; brew install httpd adds it' 'sudo apt-get install -y apache2-utils')"
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
else
  opt "Rust (rustup) was not found on your PATH (only needed to build; the prebuilt artifacts/ work without it)" \
    "fix: see https://rustup.rs, then: rustup toolchain install $RUST_VERSION --target wasm32-wasip2"
fi

section "No Docker? The local fallback (make up-local)"

# nats-server: the message bus the host listens on; also stores keyvalue and objects.
p=$(find_tool nats-server)
if [ -n "$p" ]; then
  ok "nats-server $("$p" --version 2>/dev/null | awk '{print $2}') ($p)"
else
  opt "nats-server is missing (the message bus and storage the host uses)" "fix: make tools    (only for make up-local)"
fi

# nats: the command line client; apply.py uses it to send workloads to the host.
p=$(find_tool nats)
if [ -n "$p" ]; then
  ok "nats CLI $("$p" --version 2>&1 | head -1) ($p)"
else
  opt "nats CLI is missing (used to send workloads to the host)" "fix: make tools    (only for make up-local)"
fi

# The OCI registry the host pulls components from.
if [ "$REGISTRY_MODE" = docker ]; then
  if ! command -v docker >/dev/null 2>&1; then
    opt "Docker is not installed (REGISTRY_MODE=docker runs the registry in a container)" \
      "fix: install Docker Desktop (https://docs.docker.com/get-docker/), or unset REGISTRY_MODE to use zot"
  elif ! docker info >/dev/null 2>&1; then
    opt "Docker is installed but not running" \
      "fix: $(pkg_hint 'open -a Docker' 'sudo systemctl start docker')    (then wait until it says it is running)"
  elif ! docker image inspect registry:2 >/dev/null 2>&1; then
    opt "the registry:2 image is not pulled yet" "fix: docker pull registry:2"
  else
    ok "Docker is running and registry:2 is pulled (REGISTRY_MODE=docker)"
  fi
else
  if [ -x "$TOOLS/zot" ]; then
    ok "zot in .tools/zot (the local OCI registry; no Docker needed)"
  else
    opt "zot is missing from .tools/zot (the local OCI registry components are pulled from)" \
      "fix: make tools    (only for make up-local)"
  fi
fi

# Python 3 and PyYAML: apply.py reads the workload YAML.
if ! command -v python3 >/dev/null 2>&1; then
  opt "Python 3 is missing (scripts/apply.py needs it)" \
    "fix: $(pkg_hint 'xcode-select --install    (or: brew install python)' 'sudo apt-get install -y python3 python3-yaml')"
elif ! python3 -c 'import yaml' 2>/dev/null; then
  opt "Python 3 is here, but the PyYAML package is not ($(python3 --version 2>&1))" \
    "fix: $(pkg_hint 'python3 -m pip install --user pyyaml    (Homebrew Python: brew install pyyaml)' \
                     'sudo apt-get install -y python3-yaml    (Fedora: sudo dnf install -y python3-pyyaml)')"
else
  ok "$(python3 --version 2>&1) with PyYAML"
fi

echo
if [ "$FAILS" -gt 0 ]; then
  echo "${R}Not ready:${N} $FAILS required check(s) failed. Run each 'fix:' command above, then make doctor again."
  exit 1
fi
echo "${G}Ready.${N} Next: make up, then follow steps/01-agent-http/README.md."
