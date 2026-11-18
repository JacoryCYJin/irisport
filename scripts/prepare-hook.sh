#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CARGO="${CARGO:-$HOME/.cargo/bin/cargo}"

"$CARGO" build --release --manifest-path "$ROOT/hook/Cargo.toml"
mkdir -p "$ROOT/src-tauri/bin"
install -m 755 "$ROOT/hook/target/release/irisport-hook" "$ROOT/src-tauri/bin/irisport-hook"
