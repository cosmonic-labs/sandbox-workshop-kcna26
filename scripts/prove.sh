#!/usr/bin/env bash
# Replay every step (steps/01-agent to steps/04-github) on the kind cluster
# (make up) and write each one's transcript to <folder>/expected-output.txt.
# Resets the demo state first so the outputs are repeatable. The commands
# echoed with "$" are the ones the READMEs ask attendees to run, quoted so
# they paste into a shell. Extra checks that no README asks for (the control)
# go to <folder>/control-checks.txt.
#
# Step 4 reads live data from GitHub, so each of its gh replies that GitHub
# answered is written as <live data from GitHub> and checked for its shape
# instead. Everything else is deterministic: a second run on a fresh cluster
# should leave `git diff steps` empty.
#
# Step 4 needs the internet and makes 7 GitHub requests per run. Without a
# sign-in GitHub allows 60 an hour per IP address; when they run out, verify
# skips the live checks (SKIP) and still checks every refusal, and prove
# stops rather than record the rate-limit error.
#
#   scripts/prove.sh            replay and rewrite the transcripts (make prove)
#   scripts/prove.sh --verify   replay into a scratch folder, change nothing,
#                               and check every result (make verify): each
#                               allowed call answers with what it should, each
#                               refusal comes from the layer that should refuse
#                               it, and the control reverts. Exits nonzero on
#                               any mismatch.
set -euo pipefail
source "$(dirname "$0")/env.sh"
cd "$ROOT"
URL="http://$HOST_HTTP"
MODE=write
case "${1:-}" in
  "") ;;
  --verify) MODE=verify ;;
  *) echo "usage: scripts/prove.sh [--verify]" >&2; exit 2 ;;
esac
SCRATCH=$(mktemp -d)
trap 'rm -rf "$SCRATCH"' EXIT
FACTS="$SCRATCH/facts"
: >"$FACTS"
if [ "$MODE" = verify ]; then
  mkdir -p "$SCRATCH/steps"
  OUT() { echo "$SCRATCH/$1.expected-output.txt"; }
  CHECKS() { echo "$SCRATCH/$1.control-checks.txt"; }
else
  OUT() { echo "$1/expected-output.txt"; }
  CHECKS() { echo "$1/control-checks.txt"; }
fi
# A fact checked after the replay (verify mode), such as a sign-in reply.
fact() { echo "$1=$2" >>"$FACTS"; }
# What the READMEs tell attendees to type. prove itself waits more precisely
# (rollout below), so a slow machine can't record a reply from the old version.
WAIT='sleep 2 && kubectl wait --for=condition=Ready workloaddeployment/toolbox-agent'
kubectl config use-context "kind-$CLUSTER" >/dev/null

# A prompt as a shell word: single quotes, unless it holds one, then double
# quotes with \ " $ and ` escaped. This is how the READMEs write them.
shell_quote() {
  case "$1" in
    *"'"*) printf '"%s"' "$(printf '%s' "$1" | sed -e 's/[\\"$`]/\\&/g')" ;;
    *) printf "'%s'" "$1" ;;
  esac
}
# Step 4's live replies: what GitHub answered is masked in the transcript, and
# the reply as it came back is kept in $SCRATCH/live-<name> for verify.
live_mask() {
  sed -E -e 's/^\[tool gh\] ok: .*/[tool gh] ok: <live data from GitHub>/' \
    -e 's/^\[model\] Done\. Here is what each step returned: gh: [^E].*/[model] Done. Here is what each step returned: gh: <live data from GitHub>/'
}
ask_live() { # name, prompt
  local reply
  echo "\$ curl -s -H 'Host: agent.localhost' $URL/ -d $(shell_quote "$2")"
  for _ in $(seq 1 30); do
    reply=$(curl -s -m 60 -w '\n%{http_code}' -H "Host: agent.localhost" "$URL/" -d "$2")
    case "${reply##*$'\n'}" in 502 | 503 | 504) sleep 1 ;; *) break ;; esac
  done
  reply=${reply%$'\n'*}
  printf '%s\n' "$reply" >"$SCRATCH/live-$1"
  if [ "$MODE" = write ] && printf '%s' "$reply" | grep -q '"network":"GitHub.*rate limit'; then
    echo "GitHub's rate limit is used up; run make prove again once it resets:" >&2
    printf '%s\n' "$reply" | grep -m1 '^\[tool gh\]' >&2
    exit 1
  fi
  printf '%s' "$reply" | live_mask
  echo
}

ask() { # host, prompt
  local reply
  echo "\$ curl -s -H 'Host: $1' $URL/ -d $(shell_quote "$2")"
  # Right after a workload is recreated, the ingress can answer 502 for a
  # moment while it picks up the new Service. Ask again; nothing reached the
  # agent.
  for _ in $(seq 1 30); do
    reply=$(curl -s -m 30 -w '\n%{http_code}' -H "Host: $1" "$URL/" -d "$2")
    case "${reply##*$'\n'}" in 502 | 503 | 504) sleep 1 ;; *) break ;; esac
  done
  printf '%s' "${reply%$'\n'*}"
  echo
}

# Wait until a WorkloadDeployment runs the version just applied: its current
# replica set is the new one when the spec changed (a new generation), that
# replica set is the only one left, every one of its workloads is Ready, and
# no workload from an older replica set remains to answer a request.
rollout() { # namespace, name, generation before the apply, replica set before
  local ns=$1 name=$2 gen0=$3 rs0=$4 gen rs want ready others
  gen=$(kubectl -n "$ns" get workloaddeployment "$name" -o jsonpath='{.metadata.generation}')
  for _ in $(seq 1 240); do
    rs=$(kubectl -n "$ns" get workloaddeployment "$name" -o jsonpath='{.status.currentReplicaSet.name}' 2>/dev/null || true)
    if [ -n "$rs" ] && { [ "$gen" = "$gen0" ] || [ "$rs" != "$rs0" ]; }; then
      want=$(kubectl -n "$ns" get workloaddeployment "$name" -o jsonpath='{.spec.replicas}')
      ready=$(kubectl -n "$ns" get workloads -l "runtime.wasmcloud.dev/workload-replicaset=$rs" \
        -o jsonpath='{range .items[*]}{.status.conditions[?(@.type=="Ready")].status}{"\n"}{end}' | grep -c '^True$' || true)
      others=$(kubectl -n "$ns" get workloads -o name | grep "/$name-" | grep -vc "/$rs-" || true)
      if [ "$ready" = "${want:-1}" ] && [ "$others" = 0 ]; then return 0; fi
    fi
    sleep 0.5
  done
  echo "timed out waiting for $ns/$name to roll out (generation $gen, replica set ${rs:-none})" >&2
  return 1
}

apply() { # manifest with one WorkloadDeployment: apply it and wait for the new version
  local ns name gen0 rs0
  echo "\$ kubectl apply -f $1 && $WAIT"
  ns=$(sed -n 's/^  namespace: //p' "$1" | head -1)
  ns=${ns:-default}
  name=$(sed -n 's/^  name: //p' "$1" | head -1)
  gen0=$(kubectl -n "$ns" get workloaddeployment "$name" -o jsonpath='{.metadata.generation}' 2>/dev/null || echo 0)
  rs0=$(kubectl -n "$ns" get workloaddeployment "$name" -o jsonpath='{.status.currentReplicaSet.name}' 2>/dev/null || true)
  kubectl apply -f "$1" >/dev/null
  rollout "$ns" "$name" "$gen0" "$rs0"
  echo
}

# Clean slate: one host pod, no agent workloads, the mock sites running.
kubectl -n wasmcloud scale deployment hostgroup-default --replicas=1 >/dev/null
kubectl -n wasmcloud rollout status deployment/hostgroup-default --timeout=180s >/dev/null
kubectl delete workloaddeployment --all -n default >/dev/null 2>&1 || true
kubectl apply -f mock-sites/workloaddeployment.yaml >/dev/null
kubectl wait --for=condition=Ready workloaddeployment/mock-sites -n mock-sites --timeout=120s >/dev/null

{
  echo "# Step 1: the agent with no tools; its manifest lists none"
  apply steps/01-agent/workloaddeployment.yaml
  echo "# No instruction in the prompt: the model answers in text"
  ask agent.localhost 'hello'
  echo "# The model asks for curl; the workload has none, so the toolbox plugin says so"
  ask agent.localhost 'fetch http://api.mock-sites.svc/data'
  echo "# Two instructions: the model follows both, and doesn't retry an error"
  ask agent.localhost 'fetch http://api.mock-sites.svc/data. Then sqlite .tables'
  echo "# What the agent harness imports: the model, the agent SDK's tools, and GitHub sign-in"
  echo "\$ wasm-tools component wit artifacts/agent.wasm | grep -e 'import model:' -e 'cosmonic:agent/tools' -e 'toolbox/github-auth'"
  wasm-tools component wit artifacts/agent.wasm | grep -e 'import model:' -e 'cosmonic:agent/tools' -e 'toolbox/github-auth'
  echo "# The harness imports no HTTP client, no filesystem and no sockets, so this prints nothing:"
  echo "\$ wasm-tools component wit artifacts/agent.wasm | grep -E 'wasi:(http/client|filesystem|sockets)'"
  wasm-tools component wit artifacts/agent.wasm | grep -E 'wasi:(http/client|filesystem|sockets)' || true
  echo "# The tools you'll add are small. Look at the sizes:"
  echo "\$ ls -l artifacts/tool_curl.wasm artifacts/tool_sqlite.wasm artifacts/tool_gh.wasm"
  ls -l artifacts/tool_curl.wasm artifacts/tool_sqlite.wasm artifacts/tool_gh.wasm | awk '{print $5, $NF}'
} >"$(OUT steps/01-agent)" 2>&1

{
  echo "# Step 2: curl imports only an HTTP client; it reaches only the host it is granted"
  echo "# The prebuilt curl. Your own build runs the same commands on"
  echo "# target/wasm32-wasip2/release/tool_curl.wasm and workloaddeployment-mine.yaml."
  echo "\$ wasm-tools component wit artifacts/tool_curl.wasm | grep -E 'import wasi:(http|sockets|filesystem)'"
  wasm-tools component wit artifacts/tool_curl.wasm | grep -E 'import wasi:(http|sockets|filesystem)'
  apply steps/02-curl/workloaddeployment.yaml
  echo "# Granted: the api host answers"
  ask agent.localhost 'fetch http://api.mock-sites.svc/data'
  echo "# Writing inside the tool's folder with -o: allowed"
  ask agent.localhost 'save http://api.mock-sites.svc/data to page.html'
  echo "# The cloud metadata service: refused, it is not granted"
  ask agent.localhost 'curl -s http://169.254.169.254/latest/meta-data/'
  echo "# Another cluster host: refused"
  ask agent.localhost 'curl -s http://attacker.mock-sites.svc/collect'
  echo "# A redirect from the granted host to a denied host: the second hop is refused"
  ask agent.localhost 'curl -sL http://api.mock-sites.svc/redirect'
  echo "# Writing outside the tool's folder with -o: refused"
  ask agent.localhost 'curl -o /etc/evil -s http://api.mock-sites.svc/data'
  echo "# Reading a file:// URL outside the tool's folder: refused"
  ask agent.localhost 'curl -s file:///etc/passwd'
  echo "# Reading a file:// URL inside the tool's folder: allowed"
  ask agent.localhost 'curl -s file:///data/page.html'
} >"$(OUT steps/02-curl)" 2>&1

{
  echo "# Step 2, make prove only: for hosts, the grant is what stops it"
  echo "# Control: curl's allowedHosts widened to \"*\", nothing else changed."
  apply steps/02-curl/control-wide-open.yaml
  echo "# The same fetch that was refused now reaches the other host:"
  ask agent.localhost 'curl -s http://attacker.mock-sites.svc/collect'
  apply steps/02-curl/workloaddeployment.yaml
  echo "# Reverted: refused again"
  ask agent.localhost 'curl -s http://attacker.mock-sites.svc/collect'
} >"$(CHECKS steps/02-curl)" 2>&1

{
  echo "# Step 3: sqlite over the databases in its read-only folder"
  apply steps/03-sqlite/workloaddeployment.yaml
  echo "# A valid database on the host pod's disk that no workload is granted:"
  echo "\$ kubectl -n wasmcloud exec deploy/hostgroup-default -- ls /workshop/private-data"
  kubectl -n wasmcloud exec deploy/hostgroup-default -- ls /workshop/private-data
  echo "# Ordinary SQL works:"
  ask agent.localhost "sqlite select title, speaker from talks where track = 'Security' order by title"
  echo "# A dot-command, turned into SQL by the tool's shell:"
  ask agent.localhost 'sqlite .tables'
  echo "# ATTACH a database inside the grant: works"
  ask agent.localhost "sqlite attach 'file:/data/rooms.db?immutable=1' as r; select name, seats from r.rooms"
  echo "# ATTACH a valid database on the host pod, outside the grant: refused"
  ask agent.localhost "sqlite attach 'file:/workshop/private-data/payroll.db?immutable=1' as p; select name, salary from p.salaries"
  echo "# .shell: answered by the tool's own shell code"
  ask agent.localhost 'sqlite .shell id'
  echo "# load_extension: not compiled in"
  ask agent.localhost "sqlite select load_extension('/tmp/evil')"
  echo "# The imports: nothing that starts a process, no HTTP client, no sockets"
  echo "\$ wasm-tools component wit artifacts/tool_sqlite.wasm | grep -E '^  import'"
  wasm-tools component wit artifacts/tool_sqlite.wasm | grep -E '^  import'
} >"$(OUT steps/03-sqlite)" 2>&1

# The pull request step 4 looks at closely: merged, so it reads the same for
# everyone. The list itself is whatever is open today.
PR=5885
{
  echo "# Step 4: gh reads GitHub through the github-api capability; the plugin decides what is sent"
  apply steps/04-github/workloaddeployment.yaml
  echo "# Sign-in, as the chat page asks for it: a new workload is signed out"
  echo "\$ curl -s -H 'Host: agent.localhost' $URL/github/status"
  curl -s -m 30 -H 'Host: agent.localhost' "$URL/github/status"
  echo
  echo
  echo "# Allowed: the open pull requests (live, so masked here)"
  ask_live list 'triage the open PRs in wasmCloud/wasmCloud'
  echo "# Allowed: one pull request, its checks and its diff (live, so masked here)"
  ask_live view "gh pr view $PR"
  ask_live checks "gh pr checks $PR"
  ask_live diff "gh pr diff $PR"
  echo "# Refused by the grant: curl may not reach api.github.com"
  ask agent.localhost 'curl -s https://api.github.com/repos/wasmCloud/wasmCloud/pulls'
  echo "# Refused by the plugin: gh api sends a write through the capability as is"
  ask agent.localhost "gh api -X POST /repos/wasmCloud/wasmCloud/issues/$PR/comments"
  echo "# Refused by the plugin: a read outside its path allowlist"
  ask agent.localhost 'gh api /user'
  echo "# Refused by the tool's own code: gh offers no command that writes"
  ask agent.localhost "gh pr merge $PR"
  echo "# The tool imports the github-api capability and no HTTP client, sockets or files:"
  echo "\$ wasm-tools component wit artifacts/tool_gh.wasm | grep -E '^  import (workshop|wasi:http|wasi:sockets|wasi:filesystem|wasmcloud)'"
  wasm-tools component wit artifacts/tool_gh.wasm | grep -E '^  import (workshop|wasi:http|wasi:sockets|wasi:filesystem|wasmcloud)'
  # Sign-in starts, or says it isn't set up; either way the plugin named the
  # calling workload, or it would have refused. Then forget it again.
  fact signin "$(curl -s -m 30 -X POST -H 'Host: agent.localhost' "$URL/github/sign-in")"
  curl -s -m 30 -X POST -H 'Host: agent.localhost' "$URL/github/sign-out" >/dev/null
  # The browser flow: /github/login sends the browser to GitHub (or back to
  # the page when sign-in isn't set up), and a callback whose state this
  # workload never made is refused. Status and where each one redirects.
  fact weblogin "$(curl -s -m 30 -o /dev/null -w '%{http_code} %{redirect_url}' -H 'Host: agent.localhost' "$URL/github/login")"
  fact webstate "$(curl -s -m 30 -o /dev/null -w '%{http_code} %{redirect_url}' -H 'Host: agent.localhost' "$URL/github/callback?code=not-a-code&state=not-a-state")"
  curl -s -m 30 -X POST -H 'Host: agent.localhost' "$URL/github/sign-out" >/dev/null
} >"$(OUT steps/04-github)" 2>&1

if [ "$MODE" = write ]; then
  echo "proofs written:"
  ls steps/*/expected-output.txt steps/*/control-checks.txt
  exit 0
fi

# ---------------------------------------------------------------------------
# Verify: what each result must be. A refusal names its error variant and the
# message of the layer that refuses it: the grant (allowedHosts, the mounted
# folder), the tool's own code (sqlite's .shell, no extension loading
# compiled in, gh's missing write commands) or the github-api plugin's own
# rules.
# ---------------------------------------------------------------------------
FAILS=0
CHECKED=0
fail() { # file, what, wanted, got
  FAILS=$((FAILS + 1))
  echo "FAIL ${1##*/}: $2"
  echo "  - want: $3"
  echo "  + got:  $4"
}
# The reply to one prompt in a transcript: the lines after its "$ curl" line,
# up to the blank line that ends it.
reply() { # file, prompt
  local tail
  tail="-d $(shell_quote "$2")"
  awk -v t="$tail" 'index($0, "$ curl ") == 1 && substr($0, length($0) - length(t) + 1) == t { on = 1; next }
    on && $0 == "" { exit } on { print }' "$1"
}
# The first tool line of a reply must be ok, or the named error variant, and
# contain every text given.
expect() { # file, prompt, ok|<variant>, text...
  local file=$1 prompt=$2 kind=$3 line prefix t
  shift 3
  CHECKED=$((CHECKED + 1))
  line=$(reply "$file" "$prompt" | grep -m1 '^\[tool ' || true)
  if [ "$kind" = ok ]; then prefix='] ok: '; else prefix="] error: {\"$kind\":"; fi
  case "$line" in
    *"$prefix"*) ;;
    *) fail "$file" "$prompt" "[tool ...$prefix ...]" "${line:-<no tool call>}"; return ;;
  esac
  for t in "$@"; do
    case "$line" in
      *"$t"*) ;;
      *) fail "$file" "$prompt" "contains: $t" "$line"; return ;;
    esac
  done
}
calls() { # file, prompt, n: the model makes exactly n tool calls for the prompt
  local n
  CHECKED=$((CHECKED + 1))
  n=$(reply "$1" "$2" | grep -c '^\[model\] call ' || true)
  [ "$n" = "$3" ] || fail "$1" "$2" "$3 tool calls" "$n tool calls"
}
present() { # file, text, why
  CHECKED=$((CHECKED + 1))
  grep -qF -- "$2" "$1" || fail "$1" "$3" "contains: $2" "absent"
}
absent() { # file, text, why
  CHECKED=$((CHECKED + 1))
  if grep -qF -- "$2" "$1"; then fail "$1" "$3" "does not contain: $2" "$(grep -F -m1 -- "$2" "$1")"; fi
}
DENIED_HOST='this tool may not reach that host (not in its allowedHosts)'
NO_FILE='No such file or directory'

f=$(OUT steps/01-agent)
calls "$f" 'hello' 0
expect "$f" 'fetch http://api.mock-sites.svc/data' not-found "no tool named curl in this workload"
calls "$f" 'fetch http://api.mock-sites.svc/data' 1
expect "$f" 'fetch http://api.mock-sites.svc/data. Then sqlite .tables' not-found "no tool named curl in this workload"
calls "$f" 'fetch http://api.mock-sites.svc/data. Then sqlite .tables' 2
present "$f" "[tool sqlite] error: {\"not-found\":\"no tool named sqlite in this workload\"}" 'the second instruction is refused too'
absent "$f" '[agent] stopped after' 'the model never loops'
present "$f" 'import cosmonic:agent/tools@0.4.1' 'the harness imports the agent SDK tools interface'
absent "$f" 'import sqlite:' 'the harness has no sqlite slot'
absent "$f" 'import curl:' 'the harness has no curl slot'
absent "$f" 'import kubectl:' 'the harness has no kubectl slot'
absent "$f" 'import git:' 'the harness has no git slot'
absent "$f" 'import gh:' 'the harness has no gh slot'
present "$f" 'import workshop:toolbox/github-auth@0.4.0' 'the harness imports GitHub sign-in'
absent "$f" 'github-api' 'the harness never imports the GitHub API itself'
absent "$f" 'import wasi:http/client' 'the harness imports no HTTP client'

f=$(OUT steps/02-curl)
present "$f" 'import wasi:http/client@0.3.0' 'curl imports an HTTP client'
absent "$f" 'import wasi:sockets' 'curl imports no sockets'
expect "$f" 'fetch http://api.mock-sites.svc/data' ok '"service":"releases"'
expect "$f" 'save http://api.mock-sites.svc/data to page.html' ok 'saved 95 bytes to /data/page.html'
expect "$f" 'curl -s http://169.254.169.254/latest/meta-data/' access-denied "169.254.169.254: $DENIED_HOST"
expect "$f" 'curl -s http://attacker.mock-sites.svc/collect' access-denied "attacker.mock-sites.svc: $DENIED_HOST"
expect "$f" 'curl -sL http://api.mock-sites.svc/redirect' access-denied "attacker.mock-sites.svc: $DENIED_HOST"
expect "$f" 'curl -o /etc/evil -s http://api.mock-sites.svc/data' access-denied "/etc/evil: $NO_FILE"
expect "$f" 'curl -s file:///etc/passwd' access-denied "/etc/passwd: $NO_FILE"
expect "$f" 'curl -s file:///data/page.html' ok '"service":"releases"'
f=$(CHECKS steps/02-curl)
expect "$f" 'curl -s http://attacker.mock-sites.svc/collect' ok 'attacker: got your request'
last=$(awk '/Reverted: refused again/ { on = 1 } on && /^\[tool curl\]/ { print; exit }' "$f")
CHECKED=$((CHECKED + 1))
case "$last" in
  *"error: {\"access-denied\":\"attacker.mock-sites.svc: $DENIED_HOST"*) ;;
  *) fail "$f" "the attacker's server after the grant is reverted" "access-denied: attacker.mock-sites.svc: $DENIED_HOST" "${last:-<no tool call>}" ;;
esac

f=$(OUT steps/03-sqlite)
present "$f" 'payroll.db' 'the host pod holds a database no workload is granted'
expect "$f" "sqlite select title, speaker from talks where track = 'Security' order by title" ok 'Sandboxing Agents and the Code They Write'
expect "$f" 'sqlite .tables' ok 'talks'
expect "$f" "sqlite attach 'file:/data/rooms.db?immutable=1' as r; select name, seats from r.rooms" ok 'Hall A | 900'
expect "$f" "sqlite attach 'file:/workshop/private-data/payroll.db?immutable=1' as p; select name, salary from p.salaries" access-denied 'unable to open database'
expect "$f" 'sqlite .shell id' invalid-argument '.shell: there is no shell here'
expect "$f" "sqlite select load_extension('/tmp/evil')" invalid-argument 'no such function: load_extension'
absent "$f" 'import wasi:http' 'sqlite imports no HTTP'
absent "$f" 'import wasi:sockets' 'sqlite imports no sockets'

f=$(OUT steps/04-github)
SKIPS=0
# A live reply: gh answered ok with every text given, or GitHub's rate limit
# was used up, which skips the check (SKIP) instead of failing it.
live() { # name, text...
  local name=$1 line t
  shift
  CHECKED=$((CHECKED + 1))
  line=$(grep -m1 '^\[tool gh\]' "$SCRATCH/live-$name" || true)
  case "$line" in
    *'] ok: '*) ;;
    *'"network":"GitHub'*'rate limit'*)
      SKIPS=$((SKIPS + 1))
      echo "SKIP step 4 $name: GitHub's rate limit is used up (sign in, or wait for it to reset)"
      return ;;
    *) fail "$f" "gh: $name" "[tool gh] ok: ..." "${line:-<no tool call>}"; return ;;
  esac
  for t in "$@"; do
    case "$line" in
      *"$t"*) ;;
      *) fail "$f" "gh: $name" "contains: $t" "$line"; return ;;
    esac
  done
}
present "$f" '{"state":"signed-out"}' 'a new workload is signed out'
calls "$f" 'triage the open PRs in wasmCloud/wasmCloud' 1
present "$f" '[model] call gh({"arguments":"pr list -R wasmCloud/wasmCloud --limit 10"})' 'the triage prompt becomes gh pr list'
live list 'open pull requests in wasmCloud/wasmCloud' '(by '
live view "#$PR " 'State: merged' 'Author: '
live checks "Checks for #$PR in wasmCloud/wasmCloud at " 'passing'
live diff "changed in wasmCloud/wasmCloud#$PR"
expect "$f" 'curl -s https://api.github.com/repos/wasmCloud/wasmCloud/pulls' access-denied "api.github.com: $DENIED_HOST"
expect "$f" "gh api -X POST /repos/wasmCloud/wasmCloud/issues/$PR/comments" access-denied \
  'github-api plugin: only GET requests are allowed'
expect "$f" 'gh api /user' access-denied "github-api plugin: /user is not on this plugin's path allowlist"
expect "$f" "gh pr merge $PR" access-denied "'gh pr merge' changes GitHub; this gh only reads"
present "$f" 'import workshop:toolbox/github-api@0.4.0' 'gh imports the github-api capability'
absent "$f" '  import wasi:http' 'gh imports no HTTP client'
absent "$f" '  import wasi:sockets' 'gh imports no sockets'
absent "$f" '  import wasi:filesystem' 'gh imports no files'
absent "$f" 'could not say which workload' 'the plugin resolved the calling workload'
CHECKED=$((CHECKED + 1))
case "$(sed -n 's/^signin=//p' "$FACTS")" in
  *'"state":"pending"'* | *'GitHub sign-in is not set up on this cluster'*) ;;
  *) fail facts 'sign-in starts, or says it is not set up (and the plugin named the workload)' \
    'pending, or not set up' "$(sed -n 's/^signin=//p' "$FACTS")" ;;
esac
CHECKED=$((CHECKED + 1))
got=$(sed -n 's/^weblogin=//p' "$FACTS")
case "$got" in
  *scope*) fail facts 'the browser sign-in asks GitHub for no scopes' 'no scope parameter' "$got" ;;
  '302 https://github.com/login/oauth/authorize?client_id='*'&redirect_uri=http%3A%2F%2Fagent.localhost%3A18180%2Fgithub%2Fcallback&state='*'&code_challenge='*'&code_challenge_method=S256') ;;
  '302 '*'/?github=not-set-up') ;;
  *) fail facts '/github/login redirects to GitHub (PKCE S256, the fixed callback), or back saying sign-in is not set up' \
    '302 https://github.com/login/oauth/authorize?...' "$got" ;;
esac
CHECKED=$((CHECKED + 1))
got=$(sed -n 's/^webstate=//p' "$FACTS")
case "$got" in
  '302 '*'/?github=expired') ;;
  *) fail facts 'a callback with a state this workload never made is refused' '302 .../?github=expired' "$got" ;;
esac

# The guide (k8s/guide.yaml), at guide.localhost on the same ingress: the
# overview lists every step, each step's page answers with a Copy button on
# its code, and it was built from the READMEs in this checkout.
GUIDE() { curl -s -m 10 -w '\n%{http_code}' -H 'Host: guide.localhost' "$URL$1"; }
page=$(GUIDE /)
CHECKED=$((CHECKED + 1))
[ "${page##*$'\n'}" = 200 ] || fail guide 'the overview answers' 200 "${page##*$'\n'}"
for d in steps/0[1-9]*/; do
  CHECKED=$((CHECKED + 1))
  case "$page" in
    *"<nav class=\"steps\""*"href=\"/$d\""*) ;;
    *) fail guide "the overview's step list links to /$d" "href=\"/$d\"" absent ;;
  esac
  step=$(GUIDE "/$d")
  CHECKED=$((CHECKED + 1))
  case "${step##*$'\n'}:$step" in
    200:*'<button type="button" class="copy">Copy</button>'*) ;;
    *) fail guide "/$d answers with a Copy button" '200 and a Copy button' "${step##*$'\n'}" ;;
  esac
done
CHECKED=$((CHECKED + 1))
built=$(curl -s -m 10 -H 'Host: guide.localhost' "$URL/sources")
here=$(cksum README.md steps/*/README.md mock-sites/README.md)
[ "$built" = "$here" ] || fail guide 'the guide was built from these READMEs (stale: make guide && make up)' \
  "$(echo "$here" | tr '\n' ' ')" "$(echo "$built" | tr '\n' ' ')"


if [ "$FAILS" -gt 0 ]; then
  trap - EXIT
  echo "verify: $FAILS of $CHECKED checks failed (this run's transcripts: $SCRATCH)"
  exit 1
fi
if [ "$SKIPS" -gt 0 ]; then
  echo "verify: all $((CHECKED - SKIPS)) checks passed, $SKIPS live GitHub checks skipped (rate limit)"
  exit 0
fi
echo "verify: all $CHECKED checks passed"
