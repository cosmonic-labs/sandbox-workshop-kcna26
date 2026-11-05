#!/usr/bin/env bash
# For maintainers. The SQLite databases the workshop mounts are committed as
# small prebuilt files, fixtures/<name>.db, so attendees need no sqlite3. Each
# one is built from its source, fixtures/<name>.sql, and fixtures/SHA256SUMS
# records the committed files.
#
#   scripts/fixtures-db.sh build   rebuild every .db from its .sql, rewrite SHA256SUMS
#   scripts/fixtures-db.sh check   the .db files match SHA256SUMS, and each one
#                                  holds exactly what its .sql builds
#
# Needs sqlite3. The check compares content (sqlite3 .dump), not bytes, because
# a different sqlite3 version writes a different file header for the same data.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
DBS="talks rooms payroll"

if command -v sha256sum >/dev/null 2>&1; then SUM="sha256sum"; else SUM="shasum -a 256"; fi
command -v sqlite3 >/dev/null 2>&1 || { echo "sqlite3 is needed for this maintainer step" >&2; exit 1; }

build_one() { # name, output file
  rm -f "$2"
  sqlite3 "$2" < "fixtures/$1.sql"
  sqlite3 "$2" 'vacuum'
}

case "${1:-check}" in
  build)
    for db in $DBS; do build_one "$db" "fixtures/$db.db"; done
    (for db in $DBS; do $SUM "fixtures/$db.db"; done) > fixtures/SHA256SUMS
    echo "rebuilt: $(for db in $DBS; do printf 'fixtures/%s.db ' "$db"; done)"
    echo "updated fixtures/SHA256SUMS; commit the .db files with it"
    ;;
  check)
    $SUM -c fixtures/SHA256SUMS
    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT
    for db in $DBS; do
      build_one "$db" "$tmp/$db.db"
      if ! diff <(sqlite3 "fixtures/$db.db" .dump) <(sqlite3 "$tmp/$db.db" .dump) >/dev/null; then
        echo "fixtures/$db.db does not hold what fixtures/$db.sql builds: run make fixtures-db" >&2
        exit 1
      fi
      echo "fixtures/$db.db: matches fixtures/$db.sql"
    done
    ;;
  *)
    echo "usage: $0 build|check" >&2
    exit 2
    ;;
esac
