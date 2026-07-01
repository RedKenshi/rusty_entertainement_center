#!/usr/bin/env bash
# Copy a release binary plus the minimal runtime assets to an install directory.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="${1:-/opt/rusty-entertainement-center}"
BINARY="${2:-$ROOT/target/release/rusty-entertainement-center}"

mkdir -p "$DEST/assets/icons/app"
cp "$BINARY" "$DEST/rusty-entertainement-center"
cp -R "$ROOT/assets/icons/app/." "$DEST/assets/icons/app/"

echo "Installed to $DEST (binary + assets/icons/app)"
