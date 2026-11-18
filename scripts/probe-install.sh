#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CARGO="${CARGO:-$HOME/.cargo/bin/cargo}"
SUPPORT="${IRISPORT_HOME:-$HOME/Library/Application Support/Irisport}"
BIN_DIR="$SUPPORT/bin"

mkdir -p "$BIN_DIR"

bash "$ROOT/scripts/prepare-hook.sh"
install -m 755 "$ROOT/src-tauri/bin/irisport-hook" "$BIN_DIR/irisport-hook"

cat > "$BIN_DIR/irisport-hook.sh" <<EOF
#!/bin/sh
BIN="$BIN_DIR/irisport-hook"
[ -x "\$BIN" ] || exit 0
"\$BIN" "\$@" 2>/dev/null || exit 0
exit 0
EOF
chmod 755 "$BIN_DIR/irisport-hook.sh"

python3 "$ROOT/scripts/hooks-config.py" install \
  --marker "irisport-hook.sh" \
  --command "/bin/sh \"$BIN_DIR/irisport-hook.sh\" --probe"

echo "探针记录: $SUPPORT/probe/events.jsonl"
