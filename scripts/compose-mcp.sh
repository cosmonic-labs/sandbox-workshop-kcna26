#!/usr/bin/env bash
# Compose the talks MCP toolset, the way Cosmonic Desktop composes an MCP
# server with its MCP bridge (components/talks-mcp/toolset.wac): the build's
# tool_sqlite.wasm and talks_mcp.wasm, and the published bridge, into
# mcp_talks_toolset.wasm in the same directory. scripts/artifacts.sh runs it
# in the build image after scripts/build.sh, so the toolset is checked and
# published like every other component.
#
#   scripts/compose-mcp.sh <dir> <bridge.wasm>
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
dir=$1 bridge=$2
wac compose "$ROOT/components/talks-mcp/toolset.wac" \
  --dep workshop:tool-sqlite="$dir/tool_sqlite.wasm" \
  --dep workshop:talks-mcp="$dir/talks_mcp.wasm" \
  --dep cosmonic:mcp-bridge="$bridge" \
  -o "$dir/mcp_talks_toolset.wasm"
