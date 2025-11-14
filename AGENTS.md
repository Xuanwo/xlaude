# xlaude · Agent Handbook

## 0 · 关于用户
- 用户：漩涡（Xuanwo）
- 偏好：高频上下文切换、以 git worktree 管理多条分支，并依赖 AI Agent 协助编码。

## 1 · 编程哲学
1. 程序首先服务于人类阅读，机器可执行只是附带价值。
2. 遵循各语言的惯用风格，代码应自解释。
3. 识别并消除以下坏味道：僵化、冗余、循环依赖、脆弱性、晦涩性、数据泥团、不必要的复杂性。
4. 一旦发现坏味道，立即提醒并提出改进方案。

## 2 · 语言策略

| 内容类型 | 语言 |
| --- | --- |
| 解释、讨论、沟通 | **简体中文** |
| 代码、注释、变量名、提交信息、文档示例 | **English**（技术内容中禁止出现中文字符） |

## 3 · 编码标准
- 仅在行为不明显时添加英文注释。
- 默认无需新增测试；仅在必要或显式要求时补充。
- 代码结构需保持可演进性，杜绝复制粘贴式实现。

## 4 · CLI 功能速览

| 命令 | 职责与要点 |
| --- | --- |
| `create [name]` | 仅允许在 base 分支（main/master/develop 或远端默认分支）执行；缺省名称从 BIP39 词库随机生成；自动更新 submodules 并拷贝 `CLAUDE.local.md`；可通过 `XLAUDE_NO_AUTO_OPEN` 跳过“是否立即打开”提示。 |
| `checkout <branch|pr>` | 支持分支名或 PR 号（`123`/`#123`）；缺失分支会从 `origin` fetch；PR 自动 fetch `pull/<n>/head`→`pr/<n>`；如已存在对应 worktree，会提示改为 `open`。 |
| `open [name]` | 无参数时：若当前目录为非 base worktree，直接打开；未被管理的 worktree 会询问后自动加入 state；否则进入交互式选择或接受管道输入；启动全局 `agent` 命令并继承所有环境变量。 |
| `add [name]` | 将当前 git worktree 写入 state，默认名称为分支名（斜杠会被 `-` 取代）；拒绝重复路径。 |
| `rename <old> <new>` | 仅更新 state 中的工作树别名，不触碰实际目录或分支。 |
| `list [--json]` | 按仓库分组展示路径/创建时间，并读取 Claude (`~/.claude/projects`) 与 Codex (`~/.codex/sessions` 或 `XLAUDE_CODEX_SESSIONS_DIR`) 会话，列出最近 3 条用户消息；`--json` 输出结构化字段，方便脚本消费。 |
| `dir [name]` | 输出纯路径，便于 `cd $(xlaude dir foo)`；可交互选择或接收管道输入。 |
| `delete [name]` | 自动检查未提交修改、未推送提交以及合并状态（通过 `git branch --merged` 与 `gh pr list` 双重检测），必要时多次确认；若目录已不存在则执行 `git worktree prune`；最后尝试安全删分支，不合并时再询问是否 `-D`。 |
| `clean` | 遍历所有仓库，取 `git worktree list --porcelain` 与 state 比对，移除已被手动删除的 worktree。 |
| `config` | 用 `$EDITOR` 打开 state 文件，便于手工修改 `agent` 等全局配置。 |
| `completions <shell>` | 输出 Bash/Zsh/Fish 补全脚本，内部调用隐藏命令 `complete-worktrees` 获取动态列表。 |
| `complete-worktrees [--format=simple|detailed]` | 提供简单或包含 repo/path/session 摘要的工作树清单，供补全或自定义脚本调用。 |

## 5 · Agent 与会话管理
- `state.json` 中的 `agent` 字段定义启动命令，默认 `claude --dangerously-skip-permissions`。命令按 Shell 规则分词，建议将复杂管道封装为脚本。
- 当 `agent` 的可执行名为 `codex` 且未显式给出位置参数时，xlaude 会在 `~/.codex/sessions`（或 `XLAUDE_CODEX_SESSIONS_DIR`）寻找与当前 worktree 匹配的最新会话，并自动追加 `resume <session-id>`。
- `list` 会解析 Claude JSONL 与 Codex session 目录，展示最近的用户消息与“time ago”标签，帮助判断上下文是否值得恢复。

## 6 · 状态与数据
- 状态位置：
  - macOS: `~/Library/Application Support/com.xuanwo.xlaude/state.json`
  - Linux: `~/.config/xlaude/state.json`
  - Windows: `%APPDATA%\xuanwo\xlaude\config\state.json`
- 条目键：`<repo-name>/<worktree-name>`；运行时若发现旧版本（无 `/`）会自动迁移并写回。
- `XLAUDE_CONFIG_DIR` 可重定向整个配置目录，便于测试或隔离环境。
- 创建/checkout 新 worktree 时若仓库根目录存在 `CLAUDE.local.md` 会自动复制；同时执行 `git submodule update --init --recursive` 保证依赖就位。

## 7 · 环境变量与自动化

| 变量 | 作用 |
| --- | --- |
| `XLAUDE_YES=1` | 对所有确认对话框默认为“是”；多用于脚本化删除或批量操作。 |
| `XLAUDE_NON_INTERACTIVE=1` | 禁用交互式选择，命令在无输入时直接失败或采用默认值。 |
| `XLAUDE_NO_AUTO_OPEN=1` | `create` 成功后不再提示“是否立即 open”。 |
| `XLAUDE_CONFIG_DIR=/path` | 覆盖 state/配置所在目录。 |
| `XLAUDE_CODEX_SESSIONS_DIR=/path` | 指定 Codex 会话日志位置，便于自定义同步策略。 |
| `XLAUDE_TEST_SEED=42` | 让随机工作树名在测试中可复现。 |
| `XLAUDE_TEST_MODE=1` | CI/测试专用，关闭部分交互并禁止自动打开新 worktree。 |

- 输入优先级：命令行参数 > 管道输入 > 交互式提示。例如 `echo wrong | xlaude open correct` 依然会打开 `correct`。
- 管道输入既可传名称，也可给 `smart_confirm` 提供 `y/n`，因此 `yes | xlaude delete foo` 可实现无人值守清理。

## 8 · 工作流示例
```bash
# 创建并立即开始一个功能分支
xlaude create ingestion-batch
xlaude open ingestion-batch

# 直接检出 GitHub PR #128 并分配独立 worktree
xlaude checkout 128
xlaude open pr-128

# 查看所有活跃上下文及最近对话
xlaude list

# 任务结束后清理
xlaude delete ingestion-batch
```

## 9 · 依赖提示
- Git ≥ 2.36（需要成熟的 worktree 支持）。
- Rust 工具链（用于构建或 `cargo install`）。
- Claude CLI 或自定义 agent（如 Codex）。
- `gh` CLI 可选，用于 `delete` 检测已经合并的 PR（无则自动降级，仅依赖 git）。

## 10 · 注意事项
- `create`/`checkout` 会拒绝在非 base 分支上执行，避免分支森林难以维护。
- `delete` 在当前目录即将被删除时，会先切换回主仓库以免 `worktree remove` 卡住；若目录已不在磁盘上，会提示是否仅从 state 清理。
- `list --json` 暴露精准路径、分支、创建时间、Claude/Codex 会话，可直接被脚本或 UI 消费；注意敏感信息输出。
- Shell 补全依赖隐藏命令 `complete-worktrees`，若需要自定义补全，记得使用 `--format=detailed` 以获得 repo/path/session 描述。
- 代码风格遵循“先思考、再尝试”原则：遇到不确定性需先复述问题、列出方案，再实施。
