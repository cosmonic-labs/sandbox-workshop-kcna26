#!/usr/bin/env bash
# Put the data the tools read into .run, so kind can mount it:
#
#   .run/sqlite-data/main.db   the talks database, a copy of databases/talks.db
#   .run/sqlite-data/rooms.db  a second database the sqlite tool may attach (databases/rooms.db)
#   .run/private-data/payroll.db  a database on the host pod that no workload is
#                              granted (databases/payroll.db), for step 3's ATTACH
#   .run/curl-data             an empty folder curl may write into with -o
#
# The databases are committed prebuilt (databases/*.db, built from the .sql
# files next to them by `make databases`), so this needs no sqlite3. They are
# checked against databases/SHA256SUMS before they are copied.
#
# Runs before the cluster is created (scripts/up.sh), because kind mounts these
# node paths at creation time.
set -euo pipefail
source "$(dirname "$0")/env.sh"

if command -v sha256sum >/dev/null 2>&1; then SUM="sha256sum"; else SUM="shasum -a 256"; fi
if ! (cd "$ROOT" && $SUM -c databases/SHA256SUMS >/dev/null); then
  echo "databases/*.db do not match databases/SHA256SUMS. Restore them: git checkout -- databases" >&2
  exit 1
fi

mkdir -p "$RUN_DIR/sqlite-data" "$RUN_DIR/curl-data" "$RUN_DIR/private-data"
# The host runs as uid 65532, so it can only write here if the folder is open
# to everyone. macOS file sharing hides this; Linux nodes and gVisor enforce it.
chmod 0777 "$RUN_DIR/curl-data"
cp "$ROOT/databases/talks.db" "$RUN_DIR/sqlite-data/main.db"
cp "$ROOT/databases/rooms.db" "$RUN_DIR/sqlite-data/rooms.db"
cp "$ROOT/databases/payroll.db" "$RUN_DIR/private-data/payroll.db"
echo "data      databases in $RUN_DIR"
