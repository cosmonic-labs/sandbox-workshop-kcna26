#!/usr/bin/env bash
# Run the workshop as a participant would, straight from the READMEs: every
# `sh` block in README.md and then steps/0*/README.md, in order, typed out
# and run in one shell, starting from a fresh clone. scripts/record.sh runs
# it under asciinema to make docs/walkthrough.cast.
#
#   scripts/walkthrough.sh <empty dir>
#
# It skips only GitHub sign-in (make github-sign-in and its curl calls),
# which waits for a person to enter a code on github.com; step 4 runs signed
# out. It needs Docker, Rust 1.98.1 (step 2 builds curl) and sqlite3, and
# no workshop cluster already running (make clean).
set -uo pipefail
SRC="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIR="${1:?usage: scripts/walkthrough.sh <empty dir>}"
mkdir -p "$DIR" && cd "$DIR" || exit 1

# The READMEs, from this checkout, so the commands are the ones in the tree
# being recorded. The clone's are the same when it is up to date with main.
FILES=("$SRC/README.md")
for f in "$SRC"/steps/0*/README.md; do FILES+=("$f"); done

skip() { # command
  case "$1" in
    *"make github-sign-in"* | *"/github/sign-in"* | *"/github/status"* | *"/github/sign-out"*) return 0 ;;
  esac
  return 1
}

type_out() { # text
  local i
  printf '\033[1;32m$\033[0m '
  for ((i = 0; i < ${#1}; i++)); do
    printf '%s' "${1:i:1}"
    sleep 0.02
  done
  printf '\n'
}

say() { # comment
  printf '\n\033[1;36m# %s\033[0m\n' "$1"
  sleep 1.5
}

for f in "${FILES[@]}"; do
  title=$(sed -n 's/^# //p' "$f" | head -1)
  case "$f" in
    */steps/*) say "$(basename "$(dirname "$f")"): $title" ;;
    *) say "$title" ;;
  esac
  block=""
  in_block=0
  while IFS= read -r line; do
    if [ "$in_block" = 0 ] && [ "$line" = '```sh' ]; then
      in_block=1
      block=""
    elif [ "$in_block" = 1 ] && [ "$line" = '```' ]; then
      in_block=0
      if skip "$block"; then continue; fi
      type_out "$block"
      eval "$block" </dev/null
      sleep 2
    elif [ "$in_block" = 1 ]; then
      block="${block:+$block
}$line"
    fi
  done <"$f"
done
say "Done. make clean deletes the cluster and the registry."
