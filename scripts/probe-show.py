#!/usr/bin/env python3
"""Print Irisport probe records without prompts or reply text."""

import datetime
import json
import os
from pathlib import Path

support = Path(
    os.environ.get("IRISPORT_HOME")
    or Path.home() / "Library" / "Application Support" / "Irisport"
)
probe = support / "probe"


def read(name):
    path = probe / name
    if not path.exists():
        return []
    rows = []
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line:
            try:
                rows.append(json.loads(line))
            except json.JSONDecodeError:
                continue
    return rows


def when(value: int) -> str:
    return datetime.datetime.fromtimestamp(value / 1000).strftime("%H:%M:%S")


events = read("events.jsonl")
print(f"== events.jsonl ({len(events)}) ==")
for row in events:
    print(
        f"{when(row.get('t', 0))}  "
        f"{str(row.get('event', '')):16}  "
        f"sid={str(row.get('session_id', ''))[:8]}  "
        f"turn={str(row.get('turn_id', ''))[:8]}  "
        f"source={row.get('source')}  "
        f"({row.get('evidence')})"
    )

links = read("deeplink.jsonl")
print()
print(f"== deeplink.jsonl ({len(links)}) ==")
for row in links:
    print(
        f"{when(row.get('t', 0))}  "
        f"sid={str(row.get('session_id', ''))[:8]}  "
        f"accepted={row.get('accepted')}  "
        f"exit={row.get('open_exit')}"
    )
