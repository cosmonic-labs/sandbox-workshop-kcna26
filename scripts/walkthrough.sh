#!/usr/bin/env bash
# Run the workshop as a participant would, straight from the READMEs:
# README.md and then steps/0*/README.md, in order, in one shell, starting
# from a fresh clone. scripts/record.sh runs it under asciinema to make
# docs/walkthrough.cast.
#
#   scripts/walkthrough.sh <empty dir>
#
# Every `sh` block is typed out and run. The README's own words narrate it:
# each section that runs a command is shown in order, its heading as a title
# and its paragraphs as comments, held on screen long enough to read.
# Sections with no command, tables, other code blocks and the "In the chat:"
# lead-ins are left out.
#
# It skips only GitHub sign-in (make github-sign-in and its curl calls),
# which waits for a person to enter a code on github.com; step 4 runs signed
# out. It needs Docker, Rust 1.98.1 (step 2 builds curl) and sqlite3, and
# no workshop cluster already running (make clean).
set -uo pipefail
SRC="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIR="${1:?usage: scripts/walkthrough.sh <empty dir>}"
mkdir -p "$DIR" && cd "$DIR" || exit 1
WIDTH=100

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

# Markdown to plain text: no bold markers, links as their text.
plain() {
  sed -E -e 's/\*\*//g' -e 's/\[([^]]*)\]\([^)]*\)/\1/g' <<<"$1"
}

# Hold the screen long enough to read: about 4 words a second.
pause_for() { # text
  local words
  words=$(wc -w <<<"$1")
  sleep $((words / 4 + 1))
}

title() { # heading
  printf '\n\033[1;36m# %s\033[0m\n' "$(plain "$1")"
  sleep 2
}

narrate() { # paragraph
  local text
  text=$(plain "$1")
  printf '\033[0;33m%s\033[0m\n' "$(fold -s -w "$WIDTH" <<<"$text" | sed 's/^/# /')"
  pause_for "$text"
}

type_out() { # command
  local i
  printf '\033[1;32m$\033[0m '
  for ((i = 0; i < ${#1}; i++)); do
    printf '%s' "${1:i:1}"
    sleep 0.02
  done
  printf '\n'
}

# Lead-ins that say where to type, not what happens.
lead_in() { # paragraph
  case "$1" in
    "In the chat:" | "In your terminal or Codespaces:") return 0 ;;
  esac
  return 1
}

# A section runs from one heading to the next. Only sections that run a
# command are shown: their paragraphs and commands are held until the
# section ends, then played back in order. A file's title is always shown;
# the headings above a shown section (its `##`, and any bold headings since)
# are shown before it.
titles=()
kinds=()
texts=()
runs=0
end_section() {
  local i
  if [ "$runs" = 1 ]; then
    for i in "${!titles[@]}"; do title "${titles[i]}"; done
    titles=()
    for i in "${!kinds[@]}"; do
      case "${kinds[i]}" in
        say) narrate "${texts[i]}" ;;
        run)
          type_out "${texts[i]}"
          eval "${texts[i]}" </dev/null
          sleep 2
          ;;
      esac
    done
  fi
  kinds=()
  texts=()
  runs=0
}
add() { # kind, text
  kinds+=("$1")
  texts+=("$2")
}
add_para() {
  if [ -n "$para" ] && ! lead_in "$para"; then add say "$para"; fi
  para=""
}
for f in "${FILES[@]}"; do
  para=""  # the paragraph being read
  fence="" # inside a code block: its language, or "-" for none
  block=""
  while IFS= read -r line; do
    if [ -n "$fence" ]; then
      if [ "$line" = '```' ]; then
        if [ "$fence" = sh ] && ! skip "$block"; then
          add run "$block"
          runs=1
        fi
        fence=""
        block=""
      elif [ "$fence" = sh ]; then
        block="${block:+$block
}$line"
      fi
      continue
    fi
    # A paragraph ends at a blank line, a heading, a code block, a table or
    # a new bullet.
    case "$line" in
      '```'* | '#'* | '**'*'**' | '**'*'** ' | '|'* | '' | '* '* | '- '*) add_para ;;
    esac
    case "$line" in
      '```'*)
        fence="${line#\`\`\`}"
        fence="${fence:--}"
        ;;
      '# '*)
        end_section
        titles=()
        title "${line#\# }"
        ;;
      '#'*)
        end_section
        titles=("$(sed -E 's/^#+ +//' <<<"$line")")
        ;;
      '**'*'**' | '**'*'** ')
        end_section
        titles+=("$line")
        ;;
      '|'* | '') ;;
      *) para="${para:+$para }$(sed -E 's/^ +//' <<<"$line")" ;;
    esac
  done <"$f"
  add_para
  end_section
done
title "Done. make clean deletes the cluster and the registry."
