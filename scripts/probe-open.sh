#!/usr/bin/env bash
set -euo pipefail

SESSION_ID="${1:?用法: bash scripts/probe-open.sh <session-id>}"
SUPPORT="${IRISPORT_HOME:-$HOME/Library/Application Support/Irisport}"
PROBE_DIR="$SUPPORT/probe"
mkdir -p "$PROBE_DIR"

URL="codex://threads/$SESSION_ID"

set +e
/usr/bin/open "$URL"
EXIT_CODE=$?
set -e

SESSION_ID="$SESSION_ID" EXIT_CODE="$EXIT_CODE" PROBE_DIR="$PROBE_DIR" python3 - <<'PY'
import json
import os
import time
from pathlib import Path

record = {
    "t": int(time.time() * 1000),
    "session_id": os.environ["SESSION_ID"],
    "accepted": os.environ["EXIT_CODE"] == "0",
    "open_exit": int(os.environ["EXIT_CODE"]),
}
path = Path(os.environ["PROBE_DIR"]) / "deeplink.jsonl"
with path.open("a", encoding="utf-8") as handle:
    handle.write(json.dumps(record, ensure_ascii=False) + "\n")
print(json.dumps(record, ensure_ascii=False))
PY

echo "URL: $URL"
echo "如果 Codex 已定位到对应聊天，请回复确认；若没有，请说明实际看到的内容。"
