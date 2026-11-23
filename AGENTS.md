# Irisport

常驻 macOS 菜单栏的 AI 任务状态与结果提醒工具。接入本机 Codex 桌面端、Kimi Code 与 WorkBuddy，使用官方 hooks 记录进行中状态、工作时间与完成结果，并回到原会话。

本仓库是 `jacory-space` 工作区中的独立子仓库。外层规则见工作区 `AGENTS.md`；本项目规则优先于外层通用规则。

## 范围

- 本轮只支持 macOS、Codex 桌面端、Kimi Code 与 WorkBuddy。ChatGPT 网页、Claude Code、其他 AI 工具不在本轮范围。
- 完整对话和回复正文留在原应用。Irisport 只保存会话标识、请求短摘录、更新时间和提醒状态。
- 界面范围限于菜单栏图标、菜单和面板，以及必要的 Codex 连接说明。

## 来源适配器

- 核心只负责提醒生命周期：会话标识、结果版本、未读/已读、7 天 / 50 条保留、面板和菜单栏。
- `src-tauri/src/sources.rs` 负责来源显示名、已读能力、回跳目标；核心和面板不得直接拼 `codex://` 或读取某个来源的状态库。
- 已读能力分为可信、推断、手动、暂不支持、待定；没有可信信号时不得假装自动已读。
- 当前：Codex 使用可信已读回执与 `codex://` 原会话回跳；Kimi Code 与 WorkBuddy 不提供原会话回跳，只在原应用实际查看后通过真实未读状态完成已读，不得打开 CLI 或提供本地手动确认。
- 各来源使用官方开始、完成及中断/失败 hooks 展示任务状态；蓝点表示进行中，绿点表示已完成。Codex 额外使用 `PreModelRequest` 与 `PostModelRequest` 补足额度/API 失败没有 `Stop` 的缺口，失败信号经过 5 秒重试宽限才结束任务；WorkBuddy 使用 `UserPromptSubmit` 与 `Stop`，没有原会话回跳，也不得打开 CLI。
- 每条任务右侧显示工作时间：进行中逐秒增长，完成后固定为本轮总耗时；不显示“工作中”或“总用时”等说明文字。
- 不提供侧边预览，不读取 `wire.jsonl`，不展示或跟踪思考、待办、工具参数及执行结果。
- 进行中任务始终保留在面板；点击或 Codex 回跳都不会收起、完成或已读，完成后才进入对应来源的可信已读流程。
- Command Code 的完成事件已验证，但已读与回跳未解决；ChatGPT 网页等待官方允许信号。

## 数据与隐私

- 运行时数据统一放在 `~/Library/Application Support/Irisport/`，不写入仓库。
- 生产事件只保存请求短摘录、任务状态和计时；探针只保存事件类型、会话与轮次 ID、时间、来源判定和回跳结果。
- 已读判断只读 Codex 状态库中 `thread_read_receipts` 的 `thread_id` 与 `first_unread_turn`、WorkBuddy 状态库中对应会话的 `unread`，以及 Kimi Code Local Storage 的 `kimi-web.unread`；不读取对话内容。
- 三个来源都只有在原应用实际查看聊天后才清除提醒；Irisport 面板打开、聚焦或点击任务行都不改变提醒状态。WorkBuddy 与 Kimi Code 必须先观察到外部未读，再观察到已读，默认已读状态不能清除提醒。
- 已读记录最多保留 7 天或 50 条；未读和进行中的请求不受限制。已消费事件处理完即删除，延迟日志最多 200 行。
- 不保存完整 hook JSON、请求正文、回复正文或 transcript 内容，也不读取 `wire.jsonl`。
- `~/.codex/hooks.json`、`~/.kimi-code/config.toml` 与 `~/.workbuddy/settings.json` 只由用户显式操作安装或撤销；不得覆盖其他配置、注释或 hooks。Kimi Code 配置使用保留格式的 TOML 编辑。

## 开发

- 项目内开发命令写在 `scripts/dev.sh` 和 README，不依赖外层 `scripts/dev.sh` 登记。
- 构建前端：`npm run build`
- 检查 Rust：`cargo check --manifest-path src-tauri/Cargo.toml`
- 测试 Rust：`cargo test --manifest-path src-tauri/Cargo.toml`
- 测试 hook：`cargo test --manifest-path hook/Cargo.toml`
- 构建 macOS 开发版：`npm run tauri:build`
- 修改 hook 解析、来源判定或提醒状态时，必须补充或更新对应单元测试。

## 视觉与语言

- 使用原生 macOS 菜单栏面板视觉：系统字体、Popover 材质、圆角、系统控件语义与系统强调色。
- 不使用工作区网站的 Personal Operating System 视觉系统，不使用等宽大写品牌标签、编辑式编号或细线档案风格。
- 不使用 emoji 图标；需要图标时使用 SVG 或系统图形。
- 界面语言为 zh-CN。

## Git

- 未经明确 Git 口令不提交、不推送、不创建 PR。
- 不提交运行时数据、事件日志、SQLite、构建产物或本地签名材料。
