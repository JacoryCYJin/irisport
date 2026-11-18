#!/usr/bin/env python3
"""Install or remove only Irisport entries in Codex hooks.json."""

import argparse
import json
import os
import shutil
import sys
import tempfile
import time
from pathlib import Path

EVENTS = ["UserPromptSubmit", "Stop"]


def hooks_path() -> Path:
    codex_home = os.environ.get("CODEX_HOME")
    base = Path(codex_home) if codex_home else Path.home() / ".codex"
    return base / "hooks.json"


def load(path: Path) -> dict:
    if not path.exists():
        return {}
    text = path.read_text(encoding="utf-8")
    if not text.strip():
        return {}
    data = json.loads(text)
    if not isinstance(data, dict):
        raise ValueError("hooks.json 顶层不是对象")
    return data


def write_atomic(path: Path, data: dict, mode: int) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, temp = tempfile.mkstemp(dir=str(path.parent), prefix=".hooks-", suffix=".json")
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as handle:
            json.dump(data, handle, ensure_ascii=False, indent=2)
            handle.write("\n")
        os.chmod(temp, mode)
        os.replace(temp, path)
    except Exception:
        if os.path.exists(temp):
            os.unlink(temp)
        raise


def marker_count(groups: list, marker: str) -> int:
    count = 0
    for group in groups:
        if not isinstance(group, dict):
            continue
        for handler in group.get("hooks", []) or []:
            if isinstance(handler, dict) and marker in str(handler.get("command", "")):
                count += 1
    return count


def install(marker: str, command: str) -> int:
    path = hooks_path()
    mode = 0o600
    data: dict = {}

    if path.exists():
        mode = path.stat().st_mode & 0o777
        data = load(path)
        backup = path.with_name(f"hooks.json.irisport-backup-{int(time.time())}")
        shutil.copy2(path, backup)
        print(f"备份（仅用于异常恢复）: {backup}")

    hooks = data.setdefault("hooks", {})
    if not isinstance(hooks, dict):
        raise ValueError("hooks 不是对象")

    added = 0
    for event in EVENTS:
        groups = hooks.setdefault(event, [])
        if not isinstance(groups, list):
            raise ValueError(f"hooks.{event} 不是数组")
        if marker_count(groups, marker) > 0:
            continue
        groups.append(
            {
                "hooks": [
                    {
                        "type": "command",
                        "command": command,
                        "async": True,
                        "timeout": 5,
                    }
                ]
            }
        )
        added += 1

    if added:
        write_atomic(path, data, mode)

    print(f"配置: {path}")
    print(f"新增事件条目: {added}")
    return 0


def remove(marker: str) -> int:
    path = hooks_path()
    if not path.exists():
        print(f"没有 {path}，无需移除")
        return 0

    data = load(path)
    hooks = data.get("hooks")
    if not isinstance(hooks, dict):
        print("hooks.json 没有 hooks 对象，无需移除")
        return 0

    removed = 0
    for event in list(hooks.keys()):
        groups = hooks[event]
        if not isinstance(groups, list):
            continue
        kept_groups = []
        for group in groups:
            if not isinstance(group, dict):
                kept_groups.append(group)
                continue
            handlers = group.get("hooks")
            if not isinstance(handlers, list):
                kept_groups.append(group)
                continue
            kept_handlers = [
                handler
                for handler in handlers
                if not (
                    isinstance(handler, dict)
                    and marker in str(handler.get("command", ""))
                )
            ]
            removed += len(handlers) - len(kept_handlers)
            if kept_handlers:
                group["hooks"] = kept_handlers
                kept_groups.append(group)
        if kept_groups:
            hooks[event] = kept_groups
        else:
            del hooks[event]

    if removed:
        if not hooks:
            data.pop("hooks", None)
        write_atomic(path, data, path.stat().st_mode & 0o777)

    print(f"配置: {path}")
    print(f"移除 Irisport 条目: {removed}")
    return 0


def status(marker: str) -> int:
    path = hooks_path()
    if not path.exists():
        print(f"配置: {path}（不存在）")
        return 0
    data = load(path)
    hooks = data.get("hooks", {})
    total = 0
    if isinstance(hooks, dict):
        for groups in hooks.values():
            if isinstance(groups, list):
                total += marker_count(groups, marker)
    print(f"配置: {path}")
    print(f"Irisport 条目: {total}")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="action", required=True)

    install_parser = sub.add_parser("install")
    install_parser.add_argument("--marker", required=True)
    install_parser.add_argument("--command", required=True)

    remove_parser = sub.add_parser("remove")
    remove_parser.add_argument("--marker", required=True)

    status_parser = sub.add_parser("status")
    status_parser.add_argument("--marker", required=True)

    args = parser.parse_args()
    try:
        if args.action == "install":
            return install(args.marker, args.command)
        if args.action == "remove":
            return remove(args.marker)
        return status(args.marker)
    except (ValueError, json.JSONDecodeError) as error:
        print(f"拒绝修改 hooks.json: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
