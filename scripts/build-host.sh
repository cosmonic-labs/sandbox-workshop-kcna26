#!/usr/bin/env bash
# Build the workshop's host image (maintainers): wash v2.12.0 with the
# toolbox host plugin (host/toolbox) compiled in. See host/README.md.
#
#   scripts/build-host.sh            build for this machine's platform, as $HOST_IMAGE
#   PLATFORM=linux/amd64 scripts/build-host.sh
#
# The upstream source is the v2.12.0 tag (.upstream, which up.sh also uses
# for the chart), copied to .run/host-src with host/wash-v2.12.0.patch applied:
# three lines that add the plugin crate to the workspace and register the
# plugin with the host. The first build takes a while; later ones reuse
# cargo's cache.
set -euo pipefail
source "$(dirname "$0")/env.sh"

# The upstream source: the v2.12.0 tag, the same checkout up.sh fetches for
# the chart (.upstream, gitignored).
TAG_REF="v$WASMCLOUD_VERSION"
if [ "$(git -C "$ROOT/.upstream" rev-parse -q --verify HEAD 2>/dev/null)" != \
     "$(git -C "$ROOT/.upstream" rev-parse -q --verify "$TAG_REF^{commit}" 2>/dev/null || echo none)" ]; then
  rm -rf "$ROOT/.upstream"
  git init -q "$ROOT/.upstream"
  git -C "$ROOT/.upstream" fetch --quiet --depth 1 https://github.com/wasmCloud/wasmCloud \
    "refs/tags/$TAG_REF:refs/tags/$TAG_REF"
  git -C "$ROOT/.upstream" -c advice.detachedHead=false checkout --quiet "$TAG_REF^{commit}"
fi

SRC="$RUN_DIR/host-src"
rm -rf "$SRC"
mkdir -p "$SRC"
git -C "$ROOT/.upstream" archive HEAD | tar -x -C "$SRC"
git -C "$SRC" init -q 2>/dev/null || true
(cd "$SRC" && git apply "$ROOT/host/wash-v2.12.0.patch")
mkdir -p "$SRC/crates/workshop-toolbox/wit/deps"
cp -R "$ROOT/host/toolbox/Cargo.toml" "$ROOT/host/toolbox/src" "$SRC/crates/workshop-toolbox/"
cp "$ROOT/host/toolbox/wit/world.wit" "$SRC/crates/workshop-toolbox/wit/"
cp -R "$ROOT"/wit/deps/* "$SRC/crates/workshop-toolbox/wit/deps/"

docker buildx build ${PLATFORM:+--platform "$PLATFORM"} --load \
  --build-arg CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-6}" \
  -f "$ROOT/host/Dockerfile" -t "$HOST_IMAGE" "$SRC"
echo "built $HOST_IMAGE"
