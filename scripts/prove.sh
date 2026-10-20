#!/usr/bin/env bash
# Replay every proof against the local `wash host` stack (scripts/up.sh) and
# write each step's transcript to steps/<step>/expected-output.txt.
#
# Assumes components are pushed (scripts/push.sh). Resets the demo state
# first so the outputs are repeatable.
set -euo pipefail
source "$(dirname "$0")/env.sh"
cd "$ROOT"
N() { nats --server "$NATS_URL" "$@"; }
ask() { # host, prompt
  echo "\$ curl -s -H 'Host: $1' http://$HOST_HTTP/ -d '$2'"
  curl -s -m 30 -H "Host: $1" "http://$HOST_HTTP/" -d "$2"
  echo
}
count() {
  echo "\$ curl -s -H 'Host: sites.localhost' http://$HOST_HTTP/count"
  curl -s -H 'Host: sites.localhost' "http://$HOST_HTTP/count"
  echo
}
apply() {
  echo "\$ scripts/apply.py $*"
  python3 scripts/apply.py "$@"
  echo
}

# Clean slate: drop demo buckets and stop every workload this repo runs.
for b in agent-memory tenant-a-memory tenant-b-memory toy-collector; do N kv del "$b" -f >/dev/null 2>&1 || true; done
for b in reports tenant-a-reports tenant-b-reports; do N object del "$b" -f >/dev/null 2>&1 || true; done
python3 scripts/apply.py --delete steps/07-tenants/tenants.yaml steps/06-web/workload.yaml \
  steps/00-fixtures/toy-sites.yaml >/dev/null 2>&1 || true

{
  echo "# Step 1: the agent answers over HTTP (wash host v2.10.3)"
  apply steps/01-agent-http/workload.yaml
  ask agent.localhost 'hello'
  echo "# Every tool slot is filled by not-installed, so a tool call gets a clear error:"
  ask agent.localhost 'read my notes'
  echo "# The agent harness and model import no filesystem, no outgoing HTTP, no sockets:"
  for c in agent model_scripted; do
    echo "\$ wasm-tools component wit target/wasm32-wasip2/release/$c.wasm | grep -E 'filesystem|outgoing-handler|sockets' || echo none"
    wasm-tools component wit "target/wasm32-wasip2/release/$c.wasm" | grep -E 'filesystem|outgoing-handler|sockets' || echo "$c: none"
  done
  echo "# The agent harness's imports from this workshop: the model and four typed, labelled tool slots"
  echo "\$ wasm-tools component wit target/wasm32-wasip2/release/agent.wasm | grep 'import .*workshop:toolbox'"
  wasm-tools component wit target/wasm32-wasip2/release/agent.wasm | grep 'import .*workshop:toolbox'
} >steps/01-agent-http/expected-output.txt 2>&1

{
  echo "# Step 2: the memory tool persists facts across calls and across a redeploy"
  apply steps/02-memory/workload.yaml
  ask agent.localhost 'remember that my favorite color is teal'
  ask agent.localhost 'remember that my api key is sk-demo-123'
  echo "\$ scripts/apply.py --delete ... && scripts/apply.py ...   # restart the workload"
  python3 scripts/apply.py --delete steps/02-memory/workload.yaml
  python3 scripts/apply.py steps/02-memory/workload.yaml
  echo
  ask agent.localhost 'what do you remember?'
  echo "\$ nats kv ls agent-memory"
  N kv ls agent-memory
} >steps/02-memory/expected-output.txt 2>&1

{
  echo "# Step 3: planted attack 1 (read only folder, mounted for the notes tool only)"
  apply steps/03-notes/workload.yaml
  ask agent.localhost 'read my notes'
  echo "\$ cat steps/03-notes/notes/todo.txt   # unchanged"
  cat steps/03-notes/notes/todo.txt
  echo
  echo "# Control: the same manifest with readOnly: false on a scratch copy of the notes"
  rm -rf "$RUN_DIR/notes-rw" && cp -R steps/03-notes/notes "$RUN_DIR/notes-rw"
  sed -e 's/readOnly: true/readOnly: false/' -e "s#path: ./notes#path: $RUN_DIR/notes-rw#" \
    steps/03-notes/workload.yaml >"$RUN_DIR/rw.yaml"
  apply .run/rw.yaml
  ask agent.localhost 'read my notes'
  echo "\$ tail -1 .run/notes-rw/todo.txt"
  tail -1 "$RUN_DIR/notes-rw/todo.txt"
} >steps/03-notes/expected-output.txt 2>&1

{
  echo "# Step 4: the reports tool saves through wasi:blobstore (NATS object store)"
  apply steps/04-reports/workload.yaml
  ask agent.localhost 'what do you remember? Then save a report'
  echo "\$ nats object get reports report.txt"
  N object get reports report.txt -O "$RUN_DIR/report.txt" --force >/dev/null
  cat "$RUN_DIR/report.txt"
  echo
} >steps/04-reports/expected-output.txt 2>&1

{
  echo "# Step 5: model-real behind the same slot (fake OpenAI-compatible endpoint on llm.example)"
  apply steps/00-fixtures/toy-sites.yaml
  apply steps/05-real-model/workload.yaml
  ask agent.localhost 'hello'
  ask agent.localhost 'recall what you know about me'
} >steps/05-real-model/expected-output.txt 2>&1

{
  echo "# Step 6: planted attack 2 (the web tool is allowed planted.example only)"
  apply steps/06-web/workload.yaml
  curl -s -X POST -H 'Host: sites.localhost' "http://$HOST_HTTP/reset" >/dev/null
  ask agent.localhost 'summarize http://planted.example/page'
  count
  echo "# Control: the web tool's allowedHosts widened to \"*\", nothing else changed"
  apply steps/06-web/control-wide-open.yaml
  curl -s -X POST -H 'Host: sites.localhost' "http://$HOST_HTTP/reset" >/dev/null
  ask agent.localhost 'summarize http://planted.example/page'
  count
  echo "# Per component, not per workload: the model is allowed llm.example, the web tool is not"
  sed -e '/name: model$/{n;s#$#\n          localResources:\n            allowedHosts: [llm.example]#;}' \
    -e "s#path: ../03-notes/notes#path: $ROOT/steps/03-notes/notes#" \
    steps/06-web/workload.yaml >"$RUN_DIR/per-component.yaml"
  apply .run/per-component.yaml
  ask agent.localhost 'fetch http://llm.example/v1/chat/completions'
  python3 scripts/apply.py steps/06-web/workload.yaml >/dev/null
} >steps/06-web/expected-output.txt 2>&1

{
  echo "# Step 7: two tenants on one host"
  apply steps/07-tenants/tenants.yaml
  ask a.agent.localhost 'remember that tenant is alpha'
  ask b.agent.localhost 'remember that tenant is bravo'
  ask a.agent.localhost 'what do you remember?'
  ask b.agent.localhost 'what do you remember?'
  echo "# Caveat: tenant B's manifest names tenant A's bucket"
  apply steps/07-tenants/caveat-shared-bucket.yaml
  ask b.agent.localhost 'what do you remember?'
  python3 scripts/apply.py steps/07-tenants/tenants.yaml >/dev/null
  echo "# Load: 1000 requests, 50 concurrent, one tenant; then both tenants at once"
  printf 'hello' >"$RUN_DIR/hello.txt"
  printf 'what do you remember?' >"$RUN_DIR/recall.txt"
  echo "\$ ab -n 1000 -c 50 -p hello.txt -H 'Host: a.agent.localhost' http://$HOST_HTTP/"
  ab -q -n 1000 -c 50 -p "$RUN_DIR/hello.txt" -T text/plain -H 'Host: a.agent.localhost' \
    "http://$HOST_HTTP/" 2>&1 | grep -E 'Complete requests|Failed requests|Requests per second|  50%|  99%'
  ab -q -n 500 -c 25 -p "$RUN_DIR/recall.txt" -T text/plain -H 'Host: a.agent.localhost' \
    "http://$HOST_HTTP/" >"$RUN_DIR/ab-a.txt" 2>&1 &
  ab -q -n 500 -c 25 -p "$RUN_DIR/recall.txt" -T text/plain -H 'Host: b.agent.localhost' \
    "http://$HOST_HTTP/" >"$RUN_DIR/ab-b.txt" 2>&1
  wait
  for t in a b; do
    echo "\$ ab -n 500 -c 25 -p recall.txt -H 'Host: $t.agent.localhost' ...   (both tenants concurrently)"
    grep -E 'Complete requests|Failed requests|Requests per second|  50%|  99%' "$RUN_DIR/ab-$t.txt"
  done
} >steps/07-tenants/expected-output.txt 2>&1

echo "proofs written:"
ls steps/*/expected-output.txt
