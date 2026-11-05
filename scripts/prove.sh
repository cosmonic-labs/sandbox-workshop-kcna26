#!/usr/bin/env bash
# Replay every step on the kind cluster (make up) and write each step's
# transcript to steps/<step>/expected-output.txt. Resets the demo state first
# so the outputs are repeatable. The commands echoed with "$" are the ones the
# step READMEs ask attendees to run, quoted so they paste into a shell. Extra
# checks that no README asks for (the controls) go to
# steps/<step>/control-checks.txt.
#
# Pod names in the demo namespace are random, so they are written as
# hello-<pod>, and kubectl's ages as <age>. Everything else is deterministic:
# a second run on a fresh cluster should leave `git diff steps` empty.
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
  OUT() { echo "$SCRATCH/steps/$1.expected-output.txt"; }
  CHECKS() { echo "$SCRATCH/steps/$1.control-checks.txt"; }
  EXTRA="$SCRATCH/extras/real-model.expected-output.txt"
else
  OUT() { echo "steps/$1/expected-output.txt"; }
  CHECKS() { echo "steps/$1/control-checks.txt"; }
  EXTRA=extras/real-model/expected-output.txt
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
# Random pod names, and kubectl's ages (45s, 12m, 5h12m, 2d3h, "3 (39s ago)"),
# as placeholders.
normalize() {
  sed -E -e 's/hello-[a-z0-9]{6,10}-[a-z0-9]{5}/hello-<pod>/g' \
    -e '/^\[tool kubectl\]|^\[model\] Done/ s/(^| |\()[0-9]+[smhdy]([0-9]+[smhd])?( |\)|$)/\1<age>\3/g'
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
  echo "# Step 1: run the agent; three tools are ready and the kubectl slot waits"
  apply steps/01-run-it/workload.yaml
  echo "# sqlite reads the talks database:"
  ask agent.localhost 'list the talks in sqlite'
  echo "# git reads the repository:"
  ask agent.localhost 'show the recent commits'
  echo "# curl reaches the one host it is granted:"
  ask agent.localhost 'fetch http://api.workshop-sites.svc/data'
  echo "# The agent harness's imports from this workshop: the model and four typed, labelled tool slots"
  echo "\$ wasm-tools component wit artifacts/agent.wasm | grep 'import .*workshop:toolbox'"
  wasm-tools component wit artifacts/agent.wasm | grep 'import .*workshop:toolbox'
  echo "# The harness imports no HTTP client, no filesystem and no sockets, so this prints nothing:"
  echo "\$ wasm-tools component wit artifacts/agent.wasm | grep -E 'wasi:(http/client|filesystem|sockets)'"
  wasm-tools component wit artifacts/agent.wasm | grep -E 'wasi:(http/client|filesystem|sockets)' || true
  echo "# The tools are small. Look at the sizes:"
  echo "\$ ls -l artifacts/tool_curl.wasm artifacts/tool_sqlite.wasm artifacts/tool_git.wasm artifacts/tool_kubectl.wasm"
  ls -l artifacts/tool_curl.wasm artifacts/tool_sqlite.wasm artifacts/tool_git.wasm artifacts/tool_kubectl.wasm | awk '{print $5, $NF}'
} >"$(OUT 01-run-it)" 2>&1

{
  echo "# Step 2: the curl tool imports only an HTTP client; its reach is the grant"
  echo "# The prebuilt curl. Your own build runs the same commands on"
  echo "# target/wasm32-wasip2/release/tool_curl.wasm and workload-mine.yaml."
  echo "\$ wasm-tools component wit artifacts/tool_curl.wasm | grep -E 'import wasi:(http|sockets|filesystem)'"
  wasm-tools component wit artifacts/tool_curl.wasm | grep -E 'import wasi:(http|sockets|filesystem)'
  apply steps/02-build-curl/workload.yaml
  echo "# It reaches the one host its manifest grants:"
  ask agent.localhost 'fetch http://api.workshop-sites.svc/data'
  echo "# It saves into its one folder:"
  ask agent.localhost 'save http://api.workshop-sites.svc/data to page.html'
} >"$(OUT 02-build-curl)" 2>&1

{
  echo "# Step 3: curl reaches only the host it is granted (api.workshop-sites.svc)"
  apply steps/03-curl/workload.yaml
  echo "# Granted: the api host answers"
  ask agent.localhost 'curl -s http://api.workshop-sites.svc/data'
  echo "# The cloud metadata service: refused, it is not granted"
  ask agent.localhost 'curl -s http://169.254.169.254/latest/meta-data/'
  echo "# Another cluster host: refused"
  ask agent.localhost 'curl -s http://collector.workshop-sites.svc/collect'
  echo "# A redirect from the granted host to a denied host: the second hop is refused"
  ask agent.localhost 'curl -sL http://api.workshop-sites.svc/redirect'
  fact collector-before-escapes "$(leaks)"
  echo "# Writing outside the tool's folder with -o: refused"
  ask agent.localhost 'curl -o /etc/evil -s http://api.workshop-sites.svc/data'
  echo "# Writing inside the tool's folder with -o: allowed"
  ask agent.localhost 'curl -o page.html -s http://api.workshop-sites.svc/data'
  echo "# Reading a file:// URL outside the tool's folder: refused"
  ask agent.localhost 'curl -s file:///etc/passwd'
  echo "# Reading a file:// URL inside the tool's folder: allowed"
  ask agent.localhost 'curl -s file:///data/page.html'
  fact collector-after-escapes "$(leaks)"
} >"$(OUT 03-curl)" 2>&1

{
  echo "# Step 3, make prove only: for hosts, the grant is what stops it"
  echo "# Control: curl's allowedHosts widened to \"*\", nothing else changed."
  apply steps/03-curl/control-wide-open.yaml
  echo "# The same fetch that was refused now reaches the other host:"
  ask agent.localhost 'curl -s http://collector.workshop-sites.svc/collect'
  fact collector-after-control "$(leaks)"
  apply steps/03-curl/workload.yaml
  echo "# Reverted: refused again"
  ask agent.localhost 'curl -s http://collector.workshop-sites.svc/collect'
} >"$(CHECKS 03-curl)" 2>&1

{
  echo "# Step 4: sqlite over the databases in its read-only folder"
  apply steps/04-sqlite/workload.yaml
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
} >"$(OUT 04-sqlite)" 2>&1

{
  echo "# Step 5: git reads a hostile repository, and none of its programs run"
  echo "# The repository's config sets core.fsmonitor, core.pager, aliases, filters,"
  echo "# a textconv, diff.external and a credential helper to marker commands."
  echo "# The tool has no import that could start a process:"
  echo "\$ wasm-tools component wit artifacts/tool_git.wasm | grep -E '^  import'"
  wasm-tools component wit artifacts/tool_git.wasm | grep -E '^  import'
  apply steps/05-git/workload.yaml
  echo "# log reads commits:"
  ask agent.localhost 'git log --oneline -5'
  echo "# status reports the branch and the tip:"
  ask agent.localhost 'git status'
  echo "# blame reads line history:"
  ask agent.localhost 'git blame README.md'
  echo "# push: refused by the tool's own dispatch"
  ask agent.localhost 'git push origin main'
  fact host-markers "$(host_markers)"
} >"$(OUT 05-git)" 2>&1

{
  echo "# Step 5, native positive control: the same repository under this machine's git."
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
} >"$(CHECKS 05-git)" 2>&1

{
  echo "# Step 6: kubectl reads the demo namespace through the kube-api capability"
  apply steps/06-kubectl/workload.yaml
  echo "# Allowed (grant and RBAC): pods in demo"
  ask agent.localhost 'kubectl get pods'
  echo "# Allowed (RBAC): deployments"
  ask agent.localhost 'kubectl get deployments'
  echo "# Allowed (RBAC grants pods/log): a pod's logs"
  ask agent.localhost 'kubectl logs deployment/hello'
  echo "# Refused by the grant: curl may not reach the API server"
  ask agent.localhost 'curl -s https://kubernetes.default.svc/api/v1/namespaces/demo/pods'
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
  echo "# The tool imports the kube-api capability and no HTTP client, sockets or secrets:"
  echo "\$ wasm-tools component wit artifacts/tool_kubectl.wasm | grep -E '^  import (workshop|wasi:http|wasi:sockets|wasmcloud)'"
  wasm-tools component wit artifacts/tool_kubectl.wasm | grep -E '^  import (workshop|wasi:http|wasi:sockets|wasmcloud)'
} >"$(OUT 06-kubectl)" 2>&1

{
  echo "# Step 6 control: the Role widened to read Secrets, nothing else changed"
  echo "\$ kubectl apply -f steps/06-kubectl/control-secrets-role.yaml"
  kubectl apply -f steps/06-kubectl/control-secrets-role.yaml
  ask agent.localhost 'kubectl get secrets'
  echo "\$ kubectl apply -f k8s/kube.yaml"
  kubectl apply -f k8s/kube.yaml >/dev/null
  echo "# Reverted: refused again"
  ask agent.localhost 'kubectl get secrets'
  fact role-reads-secrets "$(kubectl -n demo get role read-only -o jsonpath='{.rules[*].resources}' | grep -c secrets || true)"
} >"$(CHECKS 06-kubectl)" 2>&1

{
  echo "# Extra: a real model behind the same slot (a fake OpenAI compatible endpoint)"
  apply extras/real-model/workload.yaml
  echo "# The model asks for a tool, the agent runs it, the model answers:"
  ask agent.localhost 'what talks are there?'
} >"$EXTRA" 2>&1

if [ "$MODE" = write ]; then
  echo "proofs written:"
  ls steps/*/expected-output.txt steps/*/control-checks.txt
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

f=$(OUT 01-run-it)
expect "$f" 'list the talks in sqlite' ok 'A Capability Model for WebAssembly'
expect "$f" 'show the recent commits' ok '72e53e6 Add gitattributes'
expect "$f" 'fetch http://api.workshop-sites.svc/data' ok '"service":"releases"'
present "$f" 'workshop:toolbox/kubectl@0.4.0' 'the harness imports the kubectl slot'
absent "$f" 'import wasi:http/client' 'the harness imports no HTTP client'

f=$(OUT 02-build-curl)
present "$f" 'import wasi:http/client@0.3.0' 'curl imports an HTTP client'
absent "$f" 'import wasi:sockets' 'curl imports no sockets'
expect "$f" 'fetch http://api.workshop-sites.svc/data' ok '"service":"releases"'
expect "$f" 'save http://api.workshop-sites.svc/data to page.html' ok 'saved 95 bytes to /data/page.html'

f=$(OUT 03-curl)
expect "$f" 'curl -s http://api.workshop-sites.svc/data' ok '"service":"releases"'
expect "$f" 'curl -s http://169.254.169.254/latest/meta-data/' access-denied "169.254.169.254: $DENIED_HOST"
expect "$f" 'curl -s http://collector.workshop-sites.svc/collect' access-denied "collector.workshop-sites.svc: $DENIED_HOST"
expect "$f" 'curl -sL http://api.workshop-sites.svc/redirect' access-denied "collector.workshop-sites.svc: $DENIED_HOST"
expect "$f" 'curl -o /etc/evil -s http://api.workshop-sites.svc/data' access-denied "/etc/evil: $NO_FILE"
expect "$f" 'curl -o page.html -s http://api.workshop-sites.svc/data' ok 'saved 95 bytes to /data/page.html'
expect "$f" 'curl -s file:///etc/passwd' access-denied "/etc/passwd: $NO_FILE"
expect "$f" 'curl -s file:///data/page.html' ok '"service":"releases"'
want_fact collector-after-escapes "$(sed -n 's/^collector-before-escapes=//p' "$FACTS")" 'no refused request reached the collector'
f=$(CHECKS 03-curl)
expect "$f" 'curl -s http://collector.workshop-sites.svc/collect' ok 'collector: thanks, that is leak #'
last=$(awk '/Reverted: refused again/ { on = 1 } on && /^\[tool curl\]/ { print; exit }' "$f")
CHECKED=$((CHECKED + 1))
case "$last" in
  *"error: {\"access-denied\":\"collector.workshop-sites.svc: $DENIED_HOST"*) ;;
  *) fail "$f" 'the collector after the grant is reverted' "access-denied: collector.workshop-sites.svc: $DENIED_HOST" "${last:-<no tool call>}" ;;
esac
grant=$(sed -n 's/^collector-after-escapes=//p' "$FACTS")
want_fact collector-after-control "$((${grant:-0} + 1))" 'the control request reached the collector once'

f=$(OUT 04-sqlite)
present "$f" 'payroll.db' 'the host pod holds a database no workload is granted'
expect "$f" "sqlite select title, speaker from talks where track = 'Security' order by title" ok 'Sandboxing Agents and the Code They Write'
expect "$f" 'sqlite .tables' ok 'talks'
expect "$f" "sqlite attach 'file:/data/rooms.db?immutable=1' as r; select name, seats from r.rooms" ok 'Hall A | 900'
expect "$f" "sqlite attach 'file:/workshop/private-data/payroll.db?immutable=1' as p; select name, salary from p.salaries" access-denied 'unable to open database'
expect "$f" 'sqlite .shell id' invalid-argument '.shell: there is no shell here'
expect "$f" "sqlite select load_extension('/tmp/evil')" invalid-argument 'no such function: load_extension'
absent "$f" 'import wasi:http' 'sqlite imports no HTTP'
absent "$f" 'import wasi:sockets' 'sqlite imports no sockets'

f=$(OUT 05-git)
absent "$f" 'import wasi:http' 'git imports no HTTP'
absent "$f" 'import wasi:sockets' 'git imports no sockets'
expect "$f" 'git log --oneline -5' ok '72e53e6 Add gitattributes' '0c5ab57 Initial commit'
expect "$f" 'git status' ok 'On branch main HEAD is 72e53e6'
expect "$f" 'git blame README.md' ok '0c5ab578 (Workshop Fixture'
expect "$f" 'git push origin main' invalid-argument "'push' changes the repository or reaches the network"
absent "$f" '/tmp/gtfobins-git-' 'the component runs leave no marker'
want_fact host-markers 0 'no marker file on the host pod after the git tool read the repository'
f=$(CHECKS 05-git)
present "$f" '/tmp/gtfobins-git-fsmonitor' 'native git ran core.fsmonitor on status (the positive control)'

f=$(OUT 06-kubectl)
expect "$f" 'kubectl get pods' ok 'NAME READY STATUS RESTARTS AGE hello-<pod> 1/1 Running 0 <age>'
expect "$f" 'kubectl get deployments' ok 'NAME READY UP-TO-DATE AVAILABLE AGE hello 2/2 2 2 <age>'
expect "$f" 'kubectl logs deployment/hello' ok 'hello from hello-<pod>'
expect "$f" 'curl -s https://kubernetes.default.svc/api/v1/namespaces/demo/pods' access-denied "kubernetes.default.svc: $DENIED_HOST"
expect "$f" 'kubectl get secrets' access-denied "$FORBIDDEN: secrets is forbidden" 'cannot list resource'
expect "$f" 'kubectl get pods -A' access-denied "$FORBIDDEN: pods is forbidden" 'at the cluster scope'
expect "$f" 'kubectl get pods -n kube-system' access-denied "$FORBIDDEN: pods is forbidden" 'kube-system'
expect "$f" 'kubectl delete --raw /api/v1/namespaces/demo/pods/hello' access-denied "$FORBIDDEN: pods" 'cannot delete resource'
expect "$f" 'kubectl auth can-i list secrets' ok '[tool kubectl] ok: no'
present "$f" 'import workshop:toolbox/kube-api@0.4.0' 'kubectl imports the kube-api capability'
absent "$f" '  import wasi:http' 'kubectl imports no HTTP client'
f=$(CHECKS 06-kubectl)
expect "$f" 'kubectl get secrets' ok 'db-password'
last=$(awk '/Reverted: refused again/ { on = 1 } on && /^\[tool kubectl\]/ { print; exit }' "$f")
CHECKED=$((CHECKED + 1))
case "$last" in
  *"error: {\"access-denied\":\"$FORBIDDEN: secrets is forbidden"*) ;;
  *) fail "$f" 'secrets after the Role is reverted' "access-denied: $FORBIDDEN: secrets is forbidden" "${last:-<no tool call>}" ;;
esac
want_fact role-reads-secrets 0 'the Role is back to k8s/kube.yaml'

expect "$EXTRA" 'what talks are there?' ok 'Sandboxing Agents and the Code They Write'

if [ "$FAILS" -gt 0 ]; then
  trap - EXIT
  echo "verify: $FAILS of $CHECKED checks failed (this run's transcripts: $SCRATCH)"
  exit 1
fi
echo "verify: all $CHECKED checks passed"
