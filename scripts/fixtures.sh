#!/usr/bin/env bash
# Put the data the tools read into .run, so kind can mount it:
#
#   .run/sqlite-data/main.db   the talks database, a copy of fixtures/talks.db
#   .run/sqlite-data/rooms.db  a second database the sqlite tool may attach (fixtures/rooms.db)
#   .run/private-data/payroll.db  a database on the host pod that no workload is
#                              granted (fixtures/payroll.db), for step 4's ATTACH
#   .run/repo                  the hostile git repository (fixtures/make-hostile-repo.sh)
#   .run/curl-data             an empty folder curl may write into with -o
#
# The databases are committed prebuilt (fixtures/*.db, built from the .sql
# files next to them by `make fixtures-db`), so this needs no sqlite3. They are
# checked against fixtures/SHA256SUMS before they are copied.
#
# Runs before the cluster is created (scripts/up.sh), because kind mounts these
# node paths at creation time.
set -euo pipefail
source "$(dirname "$0")/env.sh"

if command -v sha256sum >/dev/null 2>&1; then SUM="sha256sum"; else SUM="shasum -a 256"; fi
if ! (cd "$ROOT" && $SUM -c fixtures/SHA256SUMS >/dev/null); then
  echo "fixtures/*.db do not match fixtures/SHA256SUMS. Restore them: git checkout -- fixtures" >&2
  exit 1
fi

mkdir -p "$RUN_DIR/sqlite-data" "$RUN_DIR/curl-data" "$RUN_DIR/private-data"
# The host runs as uid 65532, so it can only write here if the folder is open
# to everyone. macOS file sharing hides this; Linux nodes and gVisor enforce it.
chmod 0777 "$RUN_DIR/curl-data"
cp "$ROOT/fixtures/talks.db" "$RUN_DIR/sqlite-data/main.db"
cp "$ROOT/fixtures/rooms.db" "$RUN_DIR/sqlite-data/rooms.db"
cp "$ROOT/fixtures/payroll.db" "$RUN_DIR/private-data/payroll.db"

# kind bind-mounts .run/repo itself, so a running cluster loses the mount if
# the folder is deleted and made again. Build the repository next to it, then
# replace only what is inside.
bash "$ROOT/fixtures/make-hostile-repo.sh" "$RUN_DIR/repo.new" >/dev/null
mkdir -p "$RUN_DIR/repo"
find "$RUN_DIR/repo" -mindepth 1 -maxdepth 1 -exec rm -rf {} +
cp -R "$RUN_DIR/repo.new/." "$RUN_DIR/repo/"
rm -rf "$RUN_DIR/repo.new"
echo "fixtures   databases and hostile repository in $RUN_DIR"
