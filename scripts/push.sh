#!/usr/bin/env bash
# Push components to the cluster's registry.
#
#   scripts/push.sh          every prebuilt artifacts/*.wasm (or SRC=<dir>), as
#                            $REGISTRY/workshop/<name>:$TAG
#   scripts/push.sh --mine   your own curl build (make build), as
#                            $REGISTRY/workshop/tool-curl:mine
#
# Your build gets its own tag, so it never replaces the prebuilt tool that
# every step uses. steps/02-curl/workloaddeployment-mine.yaml runs it.
#
# The prebuilt files are checked against artifacts/SHA256SUMS before anything
# is pushed. Each image is then given a manifest with fixed timestamps, so the
# same .wasm always has the same image digest, in any registry. That is what
# lets k8s/values.yaml pin the plugins by digest.
#
# CI publishes the same images to the repo's packages with this script:
# REGISTRY=ghcr.io, REGISTRY_REPO_PREFIX=<owner>/<repo>, and REGISTRY_USER
# and REGISTRY_PASSWORD set, which switch to HTTPS and ghcr.io's token
# endpoint. Without them, it pushes over plain HTTP with no credentials.
set -euo pipefail
source "$(dirname "$0")/env.sh"
PREFIX="${REGISTRY_REPO_PREFIX:-workshop}"
SOURCE_URL=https://github.com/cosmonic-labs/sandbox-workshop-kcna26
if [ -n "${REGISTRY_USER:-}" ]; then
  SCHEME=https
  WASH_ARGS=(--user "$REGISTRY_USER" --password "$REGISTRY_PASSWORD")
else
  SCHEME=http
  WASH_ARGS=(--insecure)
fi
# curl's credentials for the repository being pushed: none, or a bearer token.
AUTH=()

OCI_MANIFEST=application/vnd.oci.image.manifest.v1+json
WASM_CONFIG=application/vnd.wasm.config.v0+json
EPOCH=1970-01-01T00:00:00Z
if command -v sha256sum >/dev/null 2>&1; then SUM="sha256sum"; else SUM="shasum -a 256"; fi
sha() { $SUM "$1" | cut -d' ' -f1; }
size() { wc -c <"$1" | tr -d ' '; }

# A registry request, with this repository's credentials if there are any.
reg() {
  curl -fsS ${AUTH[@]+"${AUTH[@]}"} "$@"
}

login() { # repository
  AUTH=()
  [ -n "${REGISTRY_USER:-}" ] || return 0
  local token
  token=$(curl -fsS -u "$REGISTRY_USER:$REGISTRY_PASSWORD" \
    "https://$REGISTRY/token?service=$REGISTRY&scope=repository:$1:pull,push" \
    | sed -n 's/.*"token":"\([^"]*\)".*/\1/p')
  [ -n "$token" ] || { echo "no token from https://$REGISTRY/token for $1" >&2; return 1; }
  AUTH=(-H "Authorization: Bearer $token")
}

upload_blob() { # repository, file
  local loc
  loc=$(reg -X POST -D - -o /dev/null "$SCHEME://$REGISTRY/v2/$1/blobs/uploads/" \
    | tr -d '\r' | sed -n 's/^[Ll]ocation: //p')
  case "$loc" in http*) ;; *) loc="$SCHEME://$REGISTRY$loc" ;; esac
  case "$loc" in *\?*) loc="$loc&" ;; *) loc="$loc?" ;; esac
  reg -X PUT -H 'Content-Type: application/octet-stream' \
    --data-binary "@$2" "${loc}digest=sha256:$(sha "$2")" >/dev/null
}

# Push with wash, then rewrite the image's config and manifest with fixed
# timestamps. wash records the push time in both, so each push would
# otherwise get a new digest. Prints the final digest.
push_reproducible() { # file, repository, tag
  # Runs inside $(...), which doesn't inherit set -e: stop at the first
  # failure rather than print a digest for a push that didn't happen.
  set -e
  local file=$1 repo=$2 tag=$3 tmp cfg
  "$WASH" oci push "${WASH_ARGS[@]}" "$REGISTRY/$repo:$tag" "$file" >/dev/null
  login "$repo"
  tmp=$(mktemp -d)
  reg -H "Accept: $OCI_MANIFEST" "$SCHEME://$REGISTRY/v2/$repo/manifests/$tag" >"$tmp/pushed.json"
  cfg=$(grep -o '"config":{"digest":"sha256:[0-9a-f]*"' "$tmp/pushed.json" | grep -o 'sha256:[0-9a-f]*')
  reg -L "$SCHEME://$REGISTRY/v2/$repo/blobs/$cfg" \
    | sed -E "s/\"created\":\"[^\"]*\"/\"created\":\"$EPOCH\"/" >"$tmp/config.json"
  upload_blob "$repo" "$tmp/config.json"
  printf '{"annotations":{"org.opencontainers.image.created":"%s","org.opencontainers.image.source":"%s"},"config":{"digest":"sha256:%s","mediaType":"%s","size":%s},"layers":[{"digest":"sha256:%s","mediaType":"application/wasm","size":%s}],"mediaType":"%s","schemaVersion":2}' \
    "$EPOCH" "$SOURCE_URL" "$(sha "$tmp/config.json")" "$WASM_CONFIG" "$(size "$tmp/config.json")" \
    "$(sha "$file")" "$(size "$file")" "$OCI_MANIFEST" >"$tmp/manifest.json"
  reg -X PUT -H "Content-Type: $OCI_MANIFEST" --data-binary "@$tmp/manifest.json" \
    "$SCHEME://$REGISTRY/v2/$repo/manifests/$tag" >/dev/null
  echo "sha256:$(sha "$tmp/manifest.json")"
  rm -rf "$tmp"
}

if [ "${1:-}" = "--mine" ]; then
  f="$ROOT/target/wasm32-wasip2/release/tool_curl.wasm"
  if [ ! -f "$f" ]; then
    echo "no build at $f: run make build first" >&2
    exit 1
  fi
  "$WASH" oci push --insecure "$REGISTRY/workshop/tool-curl:mine" "$f" >/dev/null
  echo "pushed $REGISTRY/workshop/tool-curl:mine"
  exit 0
fi

SRC="${1:-$ROOT/artifacts}"
if [ "$(cd "$SRC" && pwd)" = "$ROOT/artifacts" ]; then
  if ! (cd "$ROOT" && $SUM -c artifacts/SHA256SUMS >/dev/null); then
    echo "artifacts/ do not match artifacts/SHA256SUMS; nothing pushed." >&2
    echo "Restore them: git checkout -- artifacts" >&2
    exit 1
  fi
fi
for f in "$SRC"/*.wasm; do
  name=$(basename "$f" .wasm | tr _ -)
  # Not `digest=$(...) || ...`: bash would then ignore set -e inside it too.
  set +e
  digest=$(push_reproducible "$f" "$PREFIX/$name" "$TAG")
  status=$?
  set -e
  if [ "$status" != 0 ]; then
    echo "pushing $f to $REGISTRY failed; nothing after it was pushed" >&2
    exit 1
  fi
  echo "pushed $REGISTRY/$PREFIX/$name:$TAG ($digest)"
done
