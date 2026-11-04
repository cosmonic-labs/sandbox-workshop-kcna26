#!/usr/bin/env bash
# Build the data the tools read, into .run, so kind can mount it. Reproducible
# from the sources in fixtures/, so nothing binary is committed:
#
#   .run/sqlite-data/main.db   the talks database (fixtures/talks.sql)
#   .run/repo                  the hostile git repository (fixtures/make-hostile-repo.sh)
#   .run/curl-data             an empty folder curl may write into with -o
#
# Runs before the cluster is created (scripts/up.sh), because kind mounts these
# node paths at creation time.
set -euo pipefail
source "$(dirname "$0")/env.sh"

mkdir -p "$RUN_DIR/sqlite-data" "$RUN_DIR/curl-data"
rm -f "$RUN_DIR/sqlite-data/main.db"
sqlite3 "$RUN_DIR/sqlite-data/main.db" < "$ROOT/fixtures/talks.sql"

bash "$ROOT/fixtures/make-hostile-repo.sh" "$RUN_DIR/repo" >/dev/null
echo "fixtures   talks database and hostile repository in $RUN_DIR"
