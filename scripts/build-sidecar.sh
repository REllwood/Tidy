#!/usr/bin/env bash
# Build the Tidy MCP sidecar and stage it for Tauri's externalBin bundler.
#
# Tauri's `externalBin: ["binaries/tidy-mcp"]` expects the file on disk to
# be named with the Rust target triple: `tidy-mcp-<triple>`. This script
# builds the release binary and copies it there. Run it before `npm run tauri build`.
set -euo pipefail

cd "$(dirname "$0")/.."

TRIPLE="$(rustc -vV | awk '/host:/{print $2}')"
OUT_DIR="src-tauri/binaries"
DEST="$OUT_DIR/tidy-mcp-$TRIPLE"

echo "Building tidy-mcp (release) for $TRIPLE"
cargo build -p tidy-mcp --release --locked

mkdir -p "$OUT_DIR"
cp "${CARGO_TARGET_DIR:-target}/release/tidy-mcp" "$DEST"
chmod +x "$DEST"
echo "Staged sidecar: $DEST"
