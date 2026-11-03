#!/usr/bin/env bash
# Build the hostile repository the git tool reads in step 5, into $1.
#
# A repository can ask git to run a program in many places: a hook, a
# config command such as core.fsmonitor or core.pager, a bang alias, a
# smudge or textconv filter, a credential helper. Native git runs several of
# them just by reading or diffing the repository. gitoxide runs none of them
# on its own, and the git tool is a component that can start no process at
# all, so these entries do nothing when the tool reads this repository.
#
# Every configured command here is the same harmless marker: it would create a
# file under /tmp if it ran. After step 5, there is no such file, which is the
# proof. The repository is built fresh each time, so nothing is committed as a
# stored .git tree.
set -euo pipefail
DEST="${1:?usage: make-hostile-repo.sh <dir>}"
MARK="/tmp/gtfobins-git"   # a marker command would write here; it never runs

rm -rf "$DEST"
mkdir -p "$DEST"
cd "$DEST"
git init -q
git config user.email fixture@example.invalid
git config user.name 'Workshop Fixture'
# Fixed commit dates, so the repository and its commit ids are the same on
# every build and the git transcripts do not change from run to run.
export GIT_AUTHOR_DATE='2026-01-02T10:00:00 +0000' GIT_COMMITTER_DATE='2026-01-02T10:00:00 +0000'

# A few ordinary commits, so log, show, diff and blame have something to read.
cat > README.md <<'EOF'
# Release notes

Version 1.2 fixes several bugs and improves performance.
EOF
mkdir -p docs
echo 'Upgrade by restarting the service.' > docs/upgrade.txt
git add -A
git commit -q -m 'Initial commit'

echo 'Version 1.3 adds the new dashboard.' >> README.md
git add -A
git commit -q -m 'Note the 1.3 dashboard'

echo 'Back up your data before upgrading.' >> docs/upgrade.txt
git add -A
git commit -q -m 'Add a backup reminder'

# The filter and textconv are wired to the tracked files, so a worktree
# checkout or a diff would invoke them under native git. Committed before the
# hostile config below, so building this fixture with native git never runs it.
cat > .gitattributes <<'EOF'
*.txt filter=evil diff=evil
*.md  diff=evil
EOF
git add .gitattributes
git commit -q -m 'Add gitattributes'

# The hostile parts, added after the last commit so native git never runs them
# while this fixture is being built. Each would run a marker command under
# native git; the git tool runs none of them. Written straight into
# .git/config and the hooks.
cat >> .git/config <<EOF
[core]
	fsmonitor = touch $MARK-fsmonitor
	pager = touch $MARK-pager
	sshCommand = touch $MARK-sshcommand
	editor = touch $MARK-editor
[alias]
	lg = !touch $MARK-alias
[filter "evil"]
	smudge = touch $MARK-smudge
	clean = touch $MARK-clean
	process = touch $MARK-filterprocess
	required = true
[diff "evil"]
	textconv = touch $MARK-textconv
	command = touch $MARK-diffcommand
[diff]
	external = touch $MARK-diffexternal
[credential]
	helper = !touch $MARK-credhelper
[gpg]
	program = touch $MARK-gpg
EOF

# Hooks that would run on ordinary operations. Marked executable.
for hook in post-checkout post-merge pre-push; do
  cat > ".git/hooks/$hook" <<EOF
#!/bin/sh
touch $MARK-hook-$hook
EOF
  chmod +x ".git/hooks/$hook"
done

echo "built hostile repository at $DEST"
