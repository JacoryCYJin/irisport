# Irisport

macOS 菜单栏 AI 结果收件箱：统一接收 Codex、Kimi Code 与 WorkBuddy 的完成结果，集中查看并回到原任务。

`Iris` 来自希腊神话中的传信者伊里斯，`port` 是接收消息和回到原会话的入口。圆环与消息点图标也对应这一含义。

## 行为

- 发送消息后任务立即以蓝点进入面板，右侧工作时间逐秒增长；完成后变为绿点并固定显示本轮总用时，成功查看后才消失。
- 工作时间只显示计时，不添加“工作中”或“总用时”等说明文字，也不展示具体思考或工具跟踪内容。
- 进行中任务始终保留；点击或 Codex 回跳不会收起任务，也不会改变其进行中或已读状态。
- Codex 使用真实轮次 ID 拦截迟到的完成、失败和取消通知；Kimi 当前 Hook 不提供可用的真实轮次 ID，不使用程序生成的时间戳阻止正常结束；Kimi 后台任务完成不会改变主对话状态。
- Codex 额度/API 失败可能没有 `Stop`；Irisport 使用模型请求失败信号补充检测，经过 5 秒重试宽限后结束任务，正常长任务不使用固定超时清理。
- Codex 支持 `codex://` 原会话回跳，只有在原应用中实际查看聊天后，才通过可信已读回执清除提醒。
- Kimi Code 与 WorkBuddy 不提供原会话回跳，不打开 CLI，也没有本地手动已读；只有在原应用查看后，并观察到真实“未读 → 已读”变化才清除提醒。
- 打开面板或聚焦窗口不会改变提醒状态。
- 新结果只在对应会话内重新标记为未读；同一会话连续结果只占一行。
- 不同来源使用独立会话标识，相同的外部会话 ID 不会互相覆盖。
- 已读记录在本地最多保留 7 天或 50 条；已读记录不显示在面板，未读和进行中的请求不受这两个上限影响。
- 已消费事件处理完即删除；延迟日志最多保留 200 行。
- 可选系统通知、通知声音、任务摘录和菜单栏未读数量；通知权限由用户显式授权。

## 来源

Irisport 核心只处理提醒生命周期；来源差异集中在 `src-tauri/src/sources.rs`，事件和状态按来源隔离。

- **Codex**：状态与完成来自官方 hooks，`PreModelRequest` / `PostModelRequest` 补足模型失败状态，已读来自 Codex 已读回执，回跳使用 `codex://threads/<id>`；hooks 定义更新后可能需要在 Codex 中重新信任。
- **Kimi Code**：使用官方 `TurnStarted`、`Stop`、`StopFailure`、`Interrupt` 与 `Notification/task.completed` hooks，支持 CLI 和桌面端共用会话；额外只读会话 `state.json` 中的真实 `cancelled` 状态，补偿遗漏的终止 Hook；不提供原会话回跳，已读只读桌面端 Local Storage 中 `kimi-web.unread` 的布尔状态及版本历史，以本轮开始时的序列号识别两次轮询之间发生的未读→已读转换。
- **WorkBuddy**：使用 `~/.workbuddy/settings.json` 中的 `UserPromptSubmit` 与 `Stop` hooks，支持任务开始、完成和计时；额外只读 `workbuddy.db` 中明确的 `completed` 状态补偿遗漏的 Stop，其他状态不猜测；不提供原会话回跳，已读只读对应会话的 `unread` 状态。
- **Command Code**：官方 Stop hook 的完成事件已验证；已读和桌面会话回跳尚未解决，暂不启用提醒。
- **ChatGPT 网页**：未找到允许且可信的完成/已读信号，保持待定。

## 配置安全

- Codex 配置：`~/.codex/hooks.json`。
- Kimi Code 配置：`~/.kimi-code/config.toml`，遵循 `KIMI_CODE_HOME`。
- WorkBuddy 配置：`~/.workbuddy/settings.json`，遵循 `WORKBUDDY_HOME`。
- 安装前备份已有配置；Kimi Code 使用保留格式和注释的 TOML 编辑。
- 只添加或移除包含 Irisport 标识的 hooks，不修改其他来源规则。

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
cargo test --manifest-path hook/Cargo.toml
bash scripts/dev.sh start
bash scripts/dev.sh status
bash scripts/dev.sh stop
```

构建可打开的开发版：

```bash
npm run tauri:build
open src-tauri/target/release/bundle/macos/Irisport.app
```
