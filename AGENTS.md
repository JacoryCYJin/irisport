# Irisport

常驻 macOS 菜单栏的 AI 任务提醒工具。首轮只接入本机 Codex 桌面端，使用 Codex 官方 hooks 记录请求与结果，并用 `codex://threads/<thread-id>` 回到原会话。

本仓库是 `jacory-space` 工作区中的独立子仓库。外层规则见工作区 `AGENTS.md`；本项目规则优先于外层通用规则。

## 范围

- 本轮只支持 macOS 与 Codex 桌面端。ChatGPT 网页、Claude Code、其他 AI 工具不在本轮范围。
- 完整对话和回复正文留在 Codex。Irisport 只保存会话标识、请求短摘录、更新时间和提醒状态。
- 界面范围限于菜单栏图标、菜单和面板，以及必要的 Codex 连接说明。

## 数据与隐私

- 运行时数据统一放在 `~/Library/Application Support/Irisport/`，不写入仓库。
- 生产事件只保存请求短摘录；探针只保存事件类型、会话与轮次 ID、时间、来源判定和回跳结果。
- 已读判断只读 Codex 状态库中 `thread_read_receipts` 的 `thread_id` 与 `first_unread_turn`，不读取对话内容。
- 不保存完整 hook JSON、请求正文、回复正文或 transcript 内容。
- `~/.codex/hooks.json` 只由用户显式操作安装或撤销；不得覆盖其他 hooks。

## 开发

- 项目内开发命令写在 `scripts/dev.sh` 和 README，不依赖外层 `scripts/dev.sh` 登记。
- 构建前端：`npm run build`
- 检查 Rust：`cargo check --manifest-path src-tauri/Cargo.toml`
- 测试 Rust：`cargo test --manifest-path src-tauri/Cargo.toml`
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
