# 支持的 coding agents

Chronolume 将 Codex、Claude Code、OpenCode 和 DeepSeek Harness 的本地记录汇入同一分析库，默认合并展示。全局 agent 筛选与工作区、提供方、模型、时间及归档筛选共同作用于总览、项目、会话、模型、热力图、工具和导出。agent 是产生记录的客户端；提供方是模型/API 的路由来源，两者独立。

## 数据目录

| Agent | 默认位置 | 自动发现与兼容格式 |
| --- | --- | --- |
| Codex | `~/.codex` | 尊重 `CODEX_HOME`；沿用 `sessions`、`archived_sessions`、`state_5.sqlite` 和 `logs_2.sqlite` |
| Claude Code | `~/.claude` | 尊重 `CLAUDE_CONFIG_DIR`；读取 `projects/**/*.jsonl`，包括旧版平铺及嵌套 `subagents/agent-*.jsonl`；跳过编排 journal 和 orphaned 文件 |
| OpenCode | `~/.local/share/opencode` | 尊重 `OPENCODE_DATA_DIR`、`XDG_DATA_HOME` 和 `OPENCODE_DB`；读取 `opencode.db`、`opencode-<channel>.db`，兼容 `session`/`session_v2` 元数据、`message/part` 与 `session_message` 表及旧 `storage/session`、`storage/message`、`storage/part` JSON |
| DeepSeek Harness | `~/.dsh` | 尊重 `DSH_HOME`；读取 `sessions/<project>/<id>/session[.vN].jsonl[.zstd]`，支持 v0–v4 原始或 Zstandard 压缩日志，同一会话只读取最高规范代号文件 |

Windows 的 `~` 是本机用户主目录，OpenCode 同样使用 `.local/share/opencode`，不套用 Windows 的 AppData 路径。`OPENCODE_DB` 可以是绝对路径或相对 OpenCode 数据目录的文件名；`:memory:` 会话无法从磁盘导入。

在 **数据 → 连接你的工作节奏 → 配置目录** 中，可为每个 agent 指定一个目录；OpenCode 也接受数据库文件的绝对路径。DeepSeek Harness 也接受自定义 session-persistence 根目录。显式设置优先于环境变量；清空输入恢复自动发现。保存后增量同步，已有历史不会因旧目录暂时不可访问而被删除。首版面向本机 Windows/macOS 数据；不自动扫描 WSL、远程机器或多个账号配置目录。

## 会话与统计口径

- 同一规范化工作区路径归为同一个项目，沿用 Windows/macOS 原有路径身份规则。新 agent 会话分别使用 `claude:`、`opencode:`、`dsh:` 命名空间，已有 Codex ID 保持不变。
- Claude Code 子 agent 单独列为子会话，用量和会话数独立计入，详情可导航到主会话；OpenCode 的原生父会话关系也会保留。源主会话已被清理时，关联可能无法打开。
- Claude 的流式重复记录按消息 ID 与请求 ID 合并，各类 Token 保留已报告的最大值，工具按调用 ID 去重。OpenCode 同时读取数据库和旧文件，同一消息优先使用数据库内容，兼容新表替代旧表；fork 中早于子会话创建时间的复制记录不再计费。
- DeepSeek Harness 按 turn/step/request attempt 合并用量；后续样本替换同请求的先前值（包括向下修正），重试独立计入。仅统计原生 `tool/call`，流式工具片段不重复计数。v0–v1 的 `seedLength` 与 v2–v4 的带 `inherited` 标记边界排除复制的父历史，普通恢复标记保留历史用量。
- 输入总量 = 新增输入 + 缓存读取 + 缓存写入；输出包含可识别的推理 Token，推理是输出的子项，不重复加入总量。缓存命中率的分母是输入总量。
- 并行会话的活跃时长累加，表示累计 agent 工作量。Codex 沿用生命周期与等待扣除规则；OpenCode 使用已完成消息的原生起止时间，同一会话内重叠区间只计一次；DeepSeek Harness 使用 step 起止时间并保留工具执行时间，原生 turn 与首个流式 token 时间用于首字延迟；没有原生区间时沿用可配置空闲间隔估算并明确标记。Claude Code 的消息间隔估算不能准确识别全部用户等待，不等同于人工投入时间。
- 首字延迟只用原生计时，无数据返回空值；不会从消息时间差补造首字延迟。没有活跃计时则 TPS 为空。

## 成本与价格

内置价格按各品牌和渠道的官方页面维护，核对日期 **2026-10-26**，单位 USD / 1M Token。当前覆盖 OpenAI、Anthropic、DeepSeek、Google Gemini、Z.AI GLM、MiniMax、Kimi、Qwen、Grok，以及 OpenCode 已核对的专属/免费型号。完整型号、参考口径、档位和链接见 [官方价格说明](pricing.md)及应用价格表；未核验型号保持未定价。

匹配顺序为实际提供方的本地价格、已知提供方别名、模型所属品牌的精确官方参考价。渠道专属价格和用户覆盖优先。原始提供方和统计数量保持不变，已确认的模型别名规范为同一模型名称，渠道无独立配置时仅借用品牌参考价，不把它当作渠道账单。

`gpt-6.1-sol` 的历史别名统一作为同一个模型：入库、筛选、会话详情、模型分组和导出使用同一名称；既有摘要通过 Schema 7 合并，保留 Token、成本、计时及永久摘要，不以别名创建独立型号。

DeepSeek 直连按请求 UTC 时间选择峰谷价，并使用已核验的 2026 中国假日表；OpenCode 的 DeepSeek 免费型号为零价，付费渠道价独立。部分 Anthropic、Gemini、MiniMax、Qwen、Grok 型号按单次输入（新增 + 缓存读 + 缓存写）选择上下文档位。已公布的 Gemini 优惠结束日也按请求时间选择价格。档位保留在事件、模型片段和永久每日摘要的计价 ID（`@offpeak`、`@long`、`@regular`）中，事件过期后重算不再按汇总 Token 数重新选档。

已有未保留请求档位的非 Codex 历史会话，会在这次价格升级后重放可访问的结构化来源；已不存在的历史源只保留标准参考估算，无法补造请求档位。Codex 继续使用既有标准短上下文参考口径。Anthropic 和 Kimi 的原生 1 小时缓存写入按输入价 2 倍计算；未识别缓存期限、路由区域、服务等级、音频单价、缓存存储时长和付费工具的记录不推造额外费用。

日志自带成本保存在独立 `reportedCostMicrousd` 字段，不替换本地估算，不当作官方账单。当前仅“检查 OpenAI 更新”执行用户主动触发的联网价格检查；其他品牌随内置已核验快照交付，应用不后台联网。

## 同步、保留与兼容边界

Codex 沿用安全字节偏移追加。Claude Code 对变化的会话文件重放结构化快照，以正确合并后续补全的流式消息；OpenCode 按会话行数、更新时间、大小及旧文件身份检测变化，对变化的会话重新抽取，支持 WAL 中尚未 checkpoint 的更新。没有变化的会话不会重新入库。新适配器的资源使用尚未做真实大规模日志 benchmark，不能直接沿用 Codex 的吞吐结论。

重放在单来源事务中替换派生记录；取消或读取失败会回滚本来源，保留前次快照。DeepSeek Harness 不解包落盘、不修复或修改源文件；未完成的尾部保留完整记录并标注部分数据，完整行或压缩校验损坏时回滚，未知版本明确报错。某个 agent 的来源检测失败会报告脱敏错误码，其他 agent 继续同步。缺失的源文件保留已有历史；清空和重新索引仍沿用原有数据维护含义。分析库的事件明细保留 90 天，永久摘要不依赖源文件仍存在。

Claude Code 默认可能清理历史日志，OpenCode 的内部结构也会升级；只能导入磁盘仍可访问且受支持的记录，不能恢复已经被源工具删除的历史。当前通过合成 JSONL、旧 JSON、多代 SQLite/WAL 与迁移用例验证。2026-10-26 的本机验收额外发现并补齐 OpenCode 2.1.3/2.1.4 的 `session_v2` 结构；真实数据库的 8 个会话在隔离分析库中成功导入，再次同步无变化，未修改源数据库。Claude Code 真实日志、macOS 真机及大规模性能仍未验收。

格式和去重设计参考 [ccusage 的 OpenCode adapter](https://github.com/ccusage/ccusage/tree/main/rust/adapters/opencode/src)、[Tokscale 的 Claude Code parser](https://github.com/junhoyeo/tokscale/blob/main/crates/tokscale-core/src/sessions/claudecode.rs) 以及 [OpenCode v1.19.4 源码](https://github.com/anomalyco/opencode/blob/v1.19.4/packages/core/src/session/sql.ts)。Claude 的存储位置和保留行为参见 [官方会话文档](https://code.claude.com/docs/en/sessions)。

DeepSeek Harness 格式与去重口径依据[官方 session persistence](https://github.com/deepseek-ai/deepseek-harness/tree/master/packages/session/session-persistence-jsonl)、[用量投影](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/llm/token-meter/src/usage-projection.ts)与[格式版本记录](https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/session-format-status.md)。原生 v0 与当前 v4 的格式分别核对本机源码和官方当前源码，测试覆盖原始/压缩文件、代际选择、继承与恢复、用量修正、重试和版本拒绝。

2026-10-26 本机 DeepSeek Harness v0 验收：7 个会话、4,109,674 Token、70 次模型请求、112 次工具调用，与独立 Zstandard/JSON 复算逐会话一致；首次和重复同步无读取/解析错误，重复同步无变化，源文件 SHA-256 不变。v1–v4 仍以合成回归验证，未声称真实新版本或 macOS 真机验收。

正式安装后的分析库另以当前目录独立复核 8 个逻辑会话（含新增空会话），同时覆盖真实 v0/v4 文件；总量仍为 4,109,674 Token、70 次请求、112 次工具调用，最新代际选择保持统计一致，旧源文件哈希保持不变。
