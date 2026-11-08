#!/usr/bin/env bash
# Record the workshop, run straight from the READMEs (scripts/walkthrough.sh),
# to docs/walkthrough.cast. Play it back with `asciinema play
# docs/walkthrough.cast`. Pauses longer than 2 seconds (make up, the builds)
# are cut to 2 seconds.
#
# Deletes the workshop cluster first and after (make clean), records in a
# fresh clone in a temporary folder, and needs asciinema plus what
# scripts/walkthrough.sh needs.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
command -v asciinema >/dev/null || { echo "record.sh needs asciinema (https://asciinema.org)" >&2; exit 1; }
./scripts/down.sh --all >/dev/null
DIR=$(mktemp -d /tmp/workshop.XXXXXX)
# A plain environment, as a participant's new terminal would have.
env -i HOME="$HOME" PATH="$PATH" TERM=xterm-256color KUBECONFIG="${KUBECONFIG:-}" \
  asciinema rec --overwrite --quiet --cols 110 --rows 32 --idle-time-limit 2 \
  --title "Sandboxing on Kubernetes with WebAssembly: the workshop, from the README" \
  --command "$ROOT/scripts/walkthrough.sh $DIR" docs/walkthrough.cast
(cd "$DIR/sandbox-workshop-kcna26" && ./scripts/down.sh --all >/dev/null) || true
rm -rf "$DIR"
echo "recorded docs/walkthrough.cast"
