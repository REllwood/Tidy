#!/usr/bin/env bash
# Build the Tidy MCP sidecar and stage it for Tauri's externalBin bundler.
#
# Tauri's `externalBin: ["binaries/appflower-mcp"]` expects the file on disk to
# be named with the Rust target triple: `appflower-mcp-<triple>`. This script
# builds the release binary and copies it there. Run it before `npm run tauri build`.
set -euo pipefail

cd "$(dirname "$0")/.."

TRIPLE="$(rustc -vV | awk '/host:/{print $2}')"
OUT_DIR="src-tauri/binaries"
DEST="$OUT_DIR/appflower-mcp-$TRIPLE"

echo "→ Building appflower-mcp (release) for $TRIPLE"
cargo build -p appflower-mcp --release

mkdir -p "$OUT_DIR"
cp "target/release/appflower-mcp" "$DEST"
chmod +x "$DEST"
echo "✓ Staged sidecar → $DEST"
