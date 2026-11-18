#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
LOG_DIR="$ROOT/.dev/logs"
CARGO="${CARGO:-$HOME/.cargo/bin/cargo}"
TAURI_BIN="$ROOT/node_modules/.bin/tauri"
APP_BIN="$ROOT/src-tauri/target/debug/irisport"

usage() {
  cat <<'EOF'
Irisport 开发指令

用法:
  bash scripts/dev.sh start     启动开发实例（Vite + Tauri）
  bash scripts/dev.sh stop      停止本仓库开发实例
  bash scripts/dev.sh restart   停止后重新启动
  bash scripts/dev.sh status    查看进程与日志路径
  bash scripts/dev.sh logs      查看最近 120 行开发日志
  bash scripts/dev.sh build     前端构建、Rust 测试和 hook 准备
  bash scripts/dev.sh app       构建可打开的 Irisport.app
  bash scripts/dev.sh list      列出可用动作
EOF
}

dev_pids() {
  pgrep -f "$ROOT/node_modules/.bin/tauri dev" || true
  pgrep -f "$ROOT/src-tauri/target/debug/irisport" || true
}

cli_error() {
  echo "[irisport] $*；使用 bash scripts/dev.sh list 查看动作" >&2
  return 2
}

case "${1:-list}" in
  start)
    mkdir -p "$LOG_DIR"
    if [[ -n "$(dev_pids)" ]]; then
      echo "[irisport] 开发实例已在运行"
      exit 0
    fi
    bash "$ROOT/scripts/prepare-hook.sh"
    (
      cd "$ROOT"
      export PATH="$HOME/.cargo/bin:$PATH"
      nohup "$TAURI_BIN" dev >"$LOG_DIR/dev.log" 2>&1 &
    )
    echo "[irisport] 已启动，日志: $LOG_DIR/dev.log"
    ;;
  stop)
    pids="$(dev_pids)"
    if [[ -z "$pids" ]]; then
      echo "[irisport] 没有本仓库开发实例"
      exit 0
    fi
    while read -r pid; do
      [[ -n "$pid" ]] || continue
      kill "$pid" 2>/dev/null || true
    done <<<"$pids"
    sleep 1
    pids="$(dev_pids)"
    while read -r pid; do
      [[ -n "$pid" ]] || continue
      kill -9 "$pid" 2>/dev/null || true
    done <<<"$pids"
    echo "[irisport] 已停止"
    ;;
  restart)
    "${BASH_SOURCE[0]}" stop
    "${BASH_SOURCE[0]}" start
    ;;
  status)
    pids="$(dev_pids)"
    if [[ -z "$pids" ]]; then
      echo "[irisport] 未运行"
    else
      echo "[irisport] 运行中:"
      echo "$pids" | sed 's/^/  /'
    fi
    echo "日志: $LOG_DIR/dev.log"
    ;;
  logs)
    tail -n 120 "$LOG_DIR/dev.log" 2>/dev/null || echo "[irisport] 暂无日志"
    ;;
  build)
    cd "$ROOT"
    npm run build
    "$CARGO" test --manifest-path "$ROOT/src-tauri/Cargo.toml"
    bash "$ROOT/scripts/prepare-hook.sh"
    echo "[irisport] 构建检查通过"
    ;;
  app)
    cd "$ROOT"
    npm run tauri:build
    echo "[irisport] 应用: $ROOT/src-tauri/target/release/bundle/macos/Irisport.app"
    ;;
  list|ls|help|-h|--help)
    usage
    ;;
  *)
    cli_error "未知动作: $1"
    ;;
esac
