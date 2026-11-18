# Irisport

macOS 菜单栏 AI 任务提醒工具。首轮接入本机 Codex 桌面端：自动记录发出的请求和收到的结果，在菜单栏提示有新结果的会话，并点击回到原聊天。

`Iris` 来自希腊神话中的传信者伊里斯，`port` 是接收消息和回到原会话的入口。

## 行为

- 面板只显示有新结果、还没在 Codex 看过的会话；按最新结果排序，可滚动查看全部。
- 只有在 Codex 中打开或查看某个聊天后，Irisport 才读取 Codex 的已读回执，让该会话从面板消失。
- 打开 Irisport 面板或点击任务行不会改变提醒状态。
- 点击任意任务行会通过 `codex://threads/<id>` 回到对应的 Codex 聊天。
- 新结果只在对应会话内重新标记为未读；同一会话连续结果只占一行。
- 已读记录在本地最多保留 7 天或 50 条；已读记录不显示在面板，未读和进行中的请求不受这两个上限影响。
- 已消费事件处理完即删除；延迟日志最多保留 200 行。

## 来源

Irisport 核心只处理提醒生命周期；来源差异集中在 `src-tauri/src/sources.rs`。

- **Codex**：完成来自官方 hooks，已读来自 Codex 已读回执，回跳使用 `codex://threads/<id>`。
- **Command Code**：官方 Stop hook 的完成事件已验证；已读和桌面会话回跳尚未解决，暂不启用提醒。
- **ChatGPT 网页**：未找到允许且可信的完成/已读信号，保持待定。

## 状态

首轮正在用真实 Codex 桌面会话验证 hooks 事件、桌面来源识别和深链接回跳。

## 数据位置

- 应用数据：`~/Library/Application Support/Irisport/`
- 事件收件箱：`~/Library/Application Support/Irisport/events/inbox/`（处理完即删除）
- SQLite：`~/Library/Application Support/Irisport/irisport.sqlite3`
- 延迟日志：`~/Library/Application Support/Irisport/logs/latency.jsonl`（最多 200 行）

完整对话和回复正文不写入 Irisport。旧版 `events/queue.jsonl` 在升级时排空并删除。

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
