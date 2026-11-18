# Irisport

macOS 菜单栏 AI 任务提醒工具。首轮接入本机 Codex 桌面端：自动记录发出的请求和收到的结果，在菜单栏提示有新结果的会话，并点击回到原聊天。

`Iris` 来自希腊神话中的传信者伊里斯，`port` 是接收消息和回到原会话的入口。

## 行为

- 在 Codex 中打开或查看某个聊天后，Irisport 读取 Codex 的已读回执，清除该会话提醒。
- 打开 Irisport 面板并短暂停留后，也会把当前提醒标记为已查看。
- 点击任意任务行仍会通过 `codex://threads/<id>` 回到对应的 Codex 聊天。
- 新结果只在对应会话内重新标记为未读；同一会话连续结果只占一行。

## 状态

首轮正在用真实 Codex 桌面会话验证 hooks 事件、桌面来源识别和深链接回跳。

## 数据位置

- 应用数据：`~/Library/Application Support/Irisport/`
- 事件队列：`~/Library/Application Support/Irisport/events/queue.jsonl`
- SQLite：`~/Library/Application Support/Irisport/irisport.sqlite3`
- 探针记录：`~/Library/Application Support/Irisport/probe/`

完整对话和回复正文不写入 Irisport。

## 开发

```bash
npm install
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
bash scripts/dev.sh start
bash scripts/dev.sh status
bash scripts/dev.sh stop
```

构建可打开的开发版：

```bash
npm run tauri:build
open src-tauri/target/release/bundle/macos/Irisport.app
```
