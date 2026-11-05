#!/usr/bin/env bash
# Replay every step (steps/01-agent to steps/04-github) and the kubectl and
# git extras (extras/kubectl, extras/git) on the kind cluster (make up) and write each one's transcript
# to <folder>/expected-output.txt. Resets the demo state first so the outputs
# are repeatable. The commands echoed with "$" are the ones the READMEs ask
# attendees to run, quoted so they paste into a shell. Extra checks that no
# README asks for (the controls) go to <folder>/control-checks.txt.
# extras/real-model is not replayed: it needs a real model, and a real model's
# answers differ from run to run.
#
# Pod names in the demo namespace are random, so they are written as
# hello-<pod>, kubectl's ages as <age> and describe's timestamps as <time>.
# Step 4 reads live data from GitHub, so each of its gh replies that GitHub
# answered is written as <live data from GitHub> and checked for its shape
# instead. Everything else is deterministic: a second run on a fresh cluster
# should leave `git diff steps extras` empty.
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
#                               it, the native control runs the repository's
#                               programs and the components don't, and both
#                               controls revert. Exits nonzero on any mismatch.
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
  mkdir -p "$SCRATCH/steps" "$SCRATCH/extras"
  OUT() { echo "$SCRATCH/$1.expected-output.txt"; }
  CHECKS() { echo "$SCRATCH/$1.control-checks.txt"; }
else
  OUT() { echo "$1/expected-output.txt"; }
  CHECKS() { echo "$1/control-checks.txt"; }
fi
# A fact checked after the replay (verify mode), such as a counter's value.
fact() { echo "$1=$2" >>"$FACTS"; }
leaks() { curl -s -m 10 -H 'Host: sites.localhost' "$URL/count" | sed -n 's/^leaks=//p'; }
host_markers() { # marker files the git tool could have left on the host pod
  kubectl -n wasmcloud exec deploy/hostgroup-default -- ls /tmp 2>/dev/null | grep -c '^gtfobins-git-' || true
}
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
# Random pod names, kubectl's ages (45s, 12m, 5h12m, 2d3h, "3 (39s ago)") and
# describe's timestamps (Thu, 05 Nov 2026 01:40:17 +0000), as placeholders.
normalize() {
  sed -E -e 's/hello-[a-z0-9]{6,10}-[a-z0-9]{5}/hello-<pod>/g' \
    -e 's/[A-Z][a-z]{2}, [0-9]{2} [A-Z][a-z]{2} [0-9]{4} [0-9]{2}:[0-9]{2}:[0-9]{2} [+-][0-9]{4}/<time>/g' \
    -e '/^\[tool kubectl\]|^\[model\] Done/ s/(^| |\()[0-9]+[smhdy]([0-9]+[smhd])?( |\)|$)/\1<age>\3/g'
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
  printf '%s' "$reply" | normalize | live_mask
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
  printf '%s' "${reply%$'\n'*}" | normalize
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

# Clean slate: one host pod, no agent workloads, the toy sites running.
kubectl -n wasmcloud scale deployment hostgroup-default --replicas=1 >/dev/null
kubectl -n wasmcloud rollout status deployment/hostgroup-default --timeout=180s >/dev/null
kubectl delete workloaddeployment --all -n default >/dev/null 2>&1 || true
"$ROOT/scripts/stores.sh" reset >/dev/null
kubectl apply -f steps/00-fixtures/toy-sites.yaml >/dev/null
kubectl wait --for=condition=Ready workloaddeployment/toy-sites -n workshop-sites --timeout=120s >/dev/null

{
  echo "# Step 1: the agent with no tools; every tool slot holds the placeholder"
  apply steps/01-agent/workload.yaml
  echo "# No instruction in the prompt: the model answers in text"
  ask agent.localhost 'hello'
  echo "# The model asks for curl; the slot holds the placeholder, which says so"
  ask agent.localhost 'fetch http://api.workshop-sites.svc/data'
  echo "# Two instructions: the model follows both, and doesn't retry an error"
  ask agent.localhost 'fetch http://api.workshop-sites.svc/data. Then kubectl get secrets'
  echo "# The agent harness's imports from this workshop: the model, five typed, labelled tool slots and GitHub sign-in"
  echo "\$ wasm-tools component wit artifacts/agent.wasm | grep 'import .*workshop:toolbox'"
  wasm-tools component wit artifacts/agent.wasm | grep 'import .*workshop:toolbox'
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
  echo "# target/wasm32-wasip2/release/tool_curl.wasm and workload-mine.yaml."
  echo "\$ wasm-tools component wit artifacts/tool_curl.wasm | grep -E 'import wasi:(http|sockets|filesystem)'"
  wasm-tools component wit artifacts/tool_curl.wasm | grep -E 'import wasi:(http|sockets|filesystem)'
  apply steps/02-curl/workload.yaml
  echo "# Granted: the api host answers"
  ask agent.localhost 'fetch http://api.workshop-sites.svc/data'
  echo "# Writing inside the tool's folder with -o: allowed"
  ask agent.localhost 'save http://api.workshop-sites.svc/data to page.html'
  fact collector-before-escapes "$(leaks)"
  echo "# The cloud metadata service: refused, it is not granted"
  ask agent.localhost 'curl -s http://169.254.169.254/latest/meta-data/'
  echo "# Another cluster host: refused"
  ask agent.localhost 'curl -s http://collector.workshop-sites.svc/collect'
  echo "# A redirect from the granted host to a denied host: the second hop is refused"
  ask agent.localhost 'curl -sL http://api.workshop-sites.svc/redirect'
  echo "# Writing outside the tool's folder with -o: refused"
  ask agent.localhost 'curl -o /etc/evil -s http://api.workshop-sites.svc/data'
  echo "# Reading a file:// URL outside the tool's folder: refused"
  ask agent.localhost 'curl -s file:///etc/passwd'
  echo "# Reading a file:// URL inside the tool's folder: allowed"
  ask agent.localhost 'curl -s file:///data/page.html'
  fact collector-after-escapes "$(leaks)"
} >"$(OUT steps/02-curl)" 2>&1

{
  echo "# Step 2, make prove only: for hosts, the grant is what stops it"
  echo "# Control: curl's allowedHosts widened to \"*\", nothing else changed."
  apply steps/02-curl/control-wide-open.yaml
  echo "# The same fetch that was refused now reaches the other host:"
  ask agent.localhost 'curl -s http://collector.workshop-sites.svc/collect'
  fact collector-after-control "$(leaks)"
  apply steps/02-curl/workload.yaml
  echo "# Reverted: refused again"
  ask agent.localhost 'curl -s http://collector.workshop-sites.svc/collect'
} >"$(CHECKS steps/02-curl)" 2>&1

{
  echo "# Step 3: sqlite over the databases in its read-only folder"
  apply steps/03-sqlite/workload.yaml
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
  apply steps/04-github/workload.yaml
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
} >"$(OUT steps/04-github)" 2>&1

{
  echo "# Extra: kubectl reads the demo namespace through the kube-api capability"
  apply extras/kubectl/workload.yaml
  echo "# Allowed (grant and RBAC): pods in demo"
  ask agent.localhost 'kubectl get pods'
  echo "# Allowed (RBAC): deployments"
  ask agent.localhost 'kubectl get deployments'
  echo "# Allowed (RBAC grants pods/log): a pod's logs"
  ask agent.localhost 'kubectl logs deployment/hello'
  echo "# Allowed (RBAC): describe, as kubectl prints it (the reply is cut at 300 characters)"
  ask agent.localhost 'kubectl describe deployment hello'
  echo "# Refused by the grant: curl may not reach the API server"
  ask agent.localhost 'curl -s https://kubernetes.default.svc/api/v1/namespaces/demo/pods'
  echo "# Refused by the grant: the plugin's token file is outside curl's /data"
  ask agent.localhost 'curl -s file:///etc/wasmcloud/host-plugin-secrets/kube-api-token/token'
  echo "# Refused by RBAC: secrets"
  ask agent.localhost 'kubectl get secrets'
  echo "# Refused by RBAC: every namespace"
  ask agent.localhost 'kubectl get pods -A'
  echo "# Refused by RBAC: kube-system"
  ask agent.localhost 'kubectl get pods -n kube-system'
  echo "# Refused by RBAC: the plugin forwards any method, the API server decides"
  ask agent.localhost 'kubectl delete --raw /api/v1/namespaces/demo/pods/hello'
  echo "# RBAC answers a question too: may agent-reader list secrets?"
  ask agent.localhost 'kubectl auth can-i list secrets'
  echo "# And may it list pods?"
  ask agent.localhost 'kubectl auth can-i list pods'
  echo "# The tool imports the kube-api capability and no HTTP client, sockets or secrets:"
  echo "\$ wasm-tools component wit artifacts/tool_kubectl.wasm | grep -E '^  import (workshop|wasi:http|wasi:sockets|wasmcloud)'"
  wasm-tools component wit artifacts/tool_kubectl.wasm | grep -E '^  import (workshop|wasi:http|wasi:sockets|wasmcloud)'
} >"$(OUT extras/kubectl)" 2>&1

{
  echo "# Extra kubectl, control: the Role widened to read Secrets, nothing else changed"
  echo "\$ kubectl apply -f extras/kubectl/control-secrets-role.yaml"
  kubectl apply -f extras/kubectl/control-secrets-role.yaml
  ask agent.localhost 'kubectl get secrets'
  echo "\$ kubectl apply -f k8s/kube.yaml"
  kubectl apply -f k8s/kube.yaml >/dev/null
  echo "# Reverted: refused again"
  ask agent.localhost 'kubectl get secrets'
  fact role-reads-secrets "$(kubectl -n demo get role read-only -o jsonpath='{.rules[*].resources}' | grep -c secrets || true)"
} >"$(CHECKS extras/kubectl)" 2>&1

{
  echo "# Extra: git reads a hostile repository, and none of its programs run"
  echo "# The repository's config sets core.fsmonitor, core.pager, aliases, filters,"
  echo "# a textconv, diff.external and a credential helper to marker commands."
  echo "# The tool has no import that could start a process:"
  echo "\$ wasm-tools component wit artifacts/tool_git.wasm | grep -E '^  import'"
  wasm-tools component wit artifacts/tool_git.wasm | grep -E '^  import'
  apply extras/git/workload.yaml
  echo "# log reads commits:"
  ask agent.localhost 'git log --oneline -5'
  echo "# status reports the branch and the tip:"
  ask agent.localhost 'git status'
  echo "# blame reads line history:"
  ask agent.localhost 'git blame README.md'
  echo "# push: refused by the tool's own dispatch"
  ask agent.localhost 'git push origin main'
  fact host-markers "$(host_markers)"
} >"$(OUT extras/git)" 2>&1

{
  echo "# Extra git, native positive control: the same repository under this machine's git."
  echo "# Output depends on the local git version ($(git --version))."
  rm -f /tmp/gtfobins-git-*
  echo "\$ git -C .run/repo status"
  git -C .run/repo status 2>&1 || true
  echo "\$ ls /tmp/gtfobins-git-*"
  ls /tmp/gtfobins-git-* 2>&1 || true
  echo "\$ git -C .run/repo diff HEAD~2 HEAD"
  git -C .run/repo diff HEAD~2 HEAD 2>&1 || true
  echo "\$ ls /tmp/gtfobins-git-*"
  ls /tmp/gtfobins-git-* 2>&1 || true
  echo "\$ rm -f /tmp/gtfobins-git-*"
  rm -f /tmp/gtfobins-git-*
} >"$(CHECKS extras/git)" 2>&1

if [ "$MODE" = write ]; then
  echo "proofs written:"
  ls steps/*/expected-output.txt steps/*/control-checks.txt extras/*/expected-output.txt extras/*/control-checks.txt
  exit 0
fi

# ---------------------------------------------------------------------------
# Verify: what each result must be. A refusal names its error variant and the
# message of the layer that refuses it: the grant (allowedHosts, the mounted
# folder), the tool's own code (git's push, sqlite's .shell, no extension
# loading compiled in) or RBAC (the API server's Forbidden).
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
want_fact() { # name, value, why
  local got
  CHECKED=$((CHECKED + 1))
  got=$(sed -n "s/^$1=//p" "$FACTS" | tail -1)
  [ "$got" = "$2" ] || fail facts "$3" "$1=$2" "$1=${got:-<unset>}"
}
DENIED_HOST='this tool may not reach that host (not in its allowedHosts)'
NO_FILE='No such file or directory'
FORBIDDEN='Error from server (Forbidden)'

f=$(OUT steps/01-agent)
calls "$f" 'hello' 0
expect "$f" 'fetch http://api.workshop-sites.svc/data' other "the curl tool isn't installed yet"
calls "$f" 'fetch http://api.workshop-sites.svc/data' 1
expect "$f" 'fetch http://api.workshop-sites.svc/data. Then kubectl get secrets' other "the curl tool isn't installed yet"
calls "$f" 'fetch http://api.workshop-sites.svc/data. Then kubectl get secrets' 2
present "$f" "[tool kubectl] error: {\"other\":\"the kubectl tool isn't installed yet\"}" 'the second instruction reaches the kubectl placeholder'
absent "$f" '[agent] stopped after' 'the model never loops'
present "$f" 'workshop:toolbox/kubectl@0.4.0' 'the harness imports the kubectl slot'
present "$f" 'import gh: workshop:toolbox/gh@0.4.0' 'the harness imports the gh slot'
present "$f" 'import workshop:toolbox/github-auth@0.4.0' 'the harness imports GitHub sign-in'
absent "$f" 'github-api' 'the harness never imports the GitHub API itself'
absent "$f" 'import wasi:http/client' 'the harness imports no HTTP client'

f=$(OUT steps/02-curl)
present "$f" 'import wasi:http/client@0.3.0' 'curl imports an HTTP client'
absent "$f" 'import wasi:sockets' 'curl imports no sockets'
expect "$f" 'fetch http://api.workshop-sites.svc/data' ok '"service":"releases"'
expect "$f" 'save http://api.workshop-sites.svc/data to page.html' ok 'saved 95 bytes to /data/page.html'
expect "$f" 'curl -s http://169.254.169.254/latest/meta-data/' access-denied "169.254.169.254: $DENIED_HOST"
expect "$f" 'curl -s http://collector.workshop-sites.svc/collect' access-denied "collector.workshop-sites.svc: $DENIED_HOST"
expect "$f" 'curl -sL http://api.workshop-sites.svc/redirect' access-denied "collector.workshop-sites.svc: $DENIED_HOST"
expect "$f" 'curl -o /etc/evil -s http://api.workshop-sites.svc/data' access-denied "/etc/evil: $NO_FILE"
expect "$f" 'curl -s file:///etc/passwd' access-denied "/etc/passwd: $NO_FILE"
expect "$f" 'curl -s file:///data/page.html' ok '"service":"releases"'
want_fact collector-after-escapes "$(sed -n 's/^collector-before-escapes=//p' "$FACTS")" 'no refused request reached the collector'
f=$(CHECKS steps/02-curl)
expect "$f" 'curl -s http://collector.workshop-sites.svc/collect' ok 'collector: thanks, that is leak #'
last=$(awk '/Reverted: refused again/ { on = 1 } on && /^\[tool curl\]/ { print; exit }' "$f")
CHECKED=$((CHECKED + 1))
case "$last" in
  *"error: {\"access-denied\":\"collector.workshop-sites.svc: $DENIED_HOST"*) ;;
  *) fail "$f" 'the collector after the grant is reverted' "access-denied: collector.workshop-sites.svc: $DENIED_HOST" "${last:-<no tool call>}" ;;
esac
grant=$(sed -n 's/^collector-after-escapes=//p' "$FACTS")
want_fact collector-after-control "$((${grant:-0} + 1))" 'the control request reached the collector once'

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

f=$(OUT extras/kubectl)
expect "$f" 'kubectl get pods' ok 'NAME READY STATUS RESTARTS AGE hello-<pod> 1/1 Running 0 <age>'
expect "$f" 'kubectl get deployments' ok 'NAME READY UP-TO-DATE AVAILABLE AGE hello 2/2 2 2 <age>'
expect "$f" 'kubectl logs deployment/hello' ok 'hello from hello-<pod>'
expect "$f" 'kubectl describe deployment hello' ok 'Name: hello Namespace: demo CreationTimestamp: <time> Labels: <none>' \
  'Selector: app=hello Replicas: 2 desired | 2 updated | 2 total | 2 available | 0 unavailable StrategyType: RollingUpdate'
calls "$f" 'kubectl describe deployment hello' 1
expect "$f" 'curl -s https://kubernetes.default.svc/api/v1/namespaces/demo/pods' access-denied "kubernetes.default.svc: $DENIED_HOST"
expect "$f" 'curl -s file:///etc/wasmcloud/host-plugin-secrets/kube-api-token/token' access-denied 'kube-api-token/token: No such file or directory' 'this tool sees only /data'
expect "$f" 'kubectl get secrets' access-denied "$FORBIDDEN: secrets is forbidden" 'cannot list resource'
expect "$f" 'kubectl get pods -A' access-denied "$FORBIDDEN: pods is forbidden" 'at the cluster scope'
expect "$f" 'kubectl get pods -n kube-system' access-denied "$FORBIDDEN: pods is forbidden" 'kube-system'
expect "$f" 'kubectl delete --raw /api/v1/namespaces/demo/pods/hello' access-denied "$FORBIDDEN: pods" 'cannot delete resource'
expect "$f" 'kubectl auth can-i list secrets' ok '[tool kubectl] ok: no'
expect "$f" 'kubectl auth can-i list pods' ok '[tool kubectl] ok: yes'
present "$f" 'import workshop:toolbox/kube-api@0.4.0' 'kubectl imports the kube-api capability'
absent "$f" '  import wasi:http' 'kubectl imports no HTTP client'
f=$(CHECKS extras/kubectl)
expect "$f" 'kubectl get secrets' ok 'db-password'
last=$(awk '/Reverted: refused again/ { on = 1 } on && /^\[tool kubectl\]/ { print; exit }' "$f")
CHECKED=$((CHECKED + 1))
case "$last" in
  *"error: {\"access-denied\":\"$FORBIDDEN: secrets is forbidden"*) ;;
  *) fail "$f" 'secrets after the Role is reverted' "access-denied: $FORBIDDEN: secrets is forbidden" "${last:-<no tool call>}" ;;
esac
want_fact role-reads-secrets 0 'the Role is back to k8s/kube.yaml'

f=$(OUT extras/git)
absent "$f" 'import wasi:http' 'git imports no HTTP'
absent "$f" 'import wasi:sockets' 'git imports no sockets'
expect "$f" 'git log --oneline -5' ok '72e53e6 Add gitattributes' '0c5ab57 Initial commit'
expect "$f" 'git status' ok 'On branch main HEAD is 72e53e6'
expect "$f" 'git blame README.md' ok '0c5ab578 (Workshop Fixture'
expect "$f" 'git push origin main' invalid-argument "'push' changes the repository or reaches the network"
absent "$f" '/tmp/gtfobins-git-' 'the component runs leave no marker'
want_fact host-markers 0 'no marker file on the host pod after the git tool read the repository'
f=$(CHECKS extras/git)
present "$f" '/tmp/gtfobins-git-fsmonitor' 'native git ran core.fsmonitor on status (the positive control)'

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
