# 支持的 coding agents

Chronolume 将 Codex、Claude Code、OpenCode、DeepSeek Harness、WorkBuddy、Z Code 和 Google Antigravity 的本地记录汇入同一分析库，默认合并展示。全局 agent 筛选与工作区、提供方、模型、时间及归档筛选共同作用于总览、项目、会话、模型、热力图、工具和导出。agent 是产生记录的客户端；提供方是模型/API 的路由来源，两者独立。

## 数据目录

| Agent | 默认位置 | 自动发现与兼容格式 |
| --- | --- | --- |
| Codex | `~/.codex` | 尊重 `CODEX_HOME`；沿用 `sessions`、`archived_sessions`、`state_5.sqlite` 和 `logs_2.sqlite` |
| Claude Code | `~/.claude` | 尊重 `CLAUDE_CONFIG_DIR`；读取 `projects/**/*.jsonl`，包括旧版平铺及嵌套 `subagents/agent-*.jsonl`；跳过编排 journal 和 orphaned 文件 |
| OpenCode | `~/.local/share/opencode` | 尊重 `OPENCODE_DATA_DIR`、`XDG_DATA_HOME` 和 `OPENCODE_DB`；读取 `opencode.db`、`opencode-<channel>.db`，兼容 `session`/`session_v2` 元数据、`message/part` 与 `session_message` 表及旧 `storage/session`、`storage/message`、`storage/part` JSON |
| DeepSeek Harness | `~/.dsh` | 尊重 `DSH_HOME`；读取 `sessions/<project>/<id>/session[.vN].jsonl[.zstd]`，支持 v0–v4 原始或 Zstandard 压缩日志，同一会话只读取最高规范代号文件 |
| WorkBuddy / WorkBuddy AI / CodeBuddy Code CLI | `~/.workbuddy`、`~/.workbuddy-ai`、`~/.codebuddy` | 同一个 agent；读取 `projects/**/*.jsonl`。桌面版只读 `workbuddy.db` 补充元数据。CLI 尊重 `CODEBUDDY_CONFIG_DIR`，三个原生目录可单独配置 |
| Google Antigravity | `~/.gemini/antigravity`、`~/.gemini/antigravity-cli`、`~/.gemini/antigravity-ide` | 分别发现桌面版、CLI 和 IDE 的会话日志，三个目录可单独指定 |
| Z Code | `~/.zcode` | 尊重 `ZCODE_HOME`；只读 `cli/db/db.sqlite` 的原生请求、工具与轮次用量表，兼容 WAL 中尚未 checkpoint 的更新 |

Windows 的 `~` 是本机用户主目录，OpenCode 同样使用 `.local/share/opencode`，不套用 Windows 的 AppData 路径。`OPENCODE_DB` 可以是绝对路径或相对 OpenCode 数据目录的文件名；`:memory:` 会话无法从磁盘导入。

在 **数据 → 连接你的工作节奏 → 配置目录** 中，可为各来源指定目录。OpenCode 和 Z Code 也接受数据库文件的绝对路径，DeepSeek Harness 也接受自定义 session-persistence 根目录。显式设置优先于环境变量，清空路径恢复自动发现。保存后增量同步，已有历史不会因旧目录暂时不可访问而被删除。首版面向本机 Windows/macOS 数据，不自动扫描 WSL、远程机器或多个账号配置目录。

## 会话与统计口径

- 工作区由 agent 和规范化路径共同标识。同一 agent 在同一路径的会话归入一个工作区，名称使用「agent 显示名-目录名或别名」，例如 `Codex-Gaos`。新 agent 会话分别使用 `claude:`、`opencode:`、`dsh:`、`workbuddy:`、`zcode:`、`antigravity:` 命名空间，已有 Codex ID 保持不变。
- Claude Code 子 agent 单独列为子会话，用量和会话数独立计入，详情可导航到主会话；OpenCode 的原生父会话关系也会保留。源主会话已被清理时，关联可能无法打开。
- Claude 的流式重复记录按消息 ID 与请求 ID 合并，各类 Token 保留已报告的最大值，工具按调用 ID 去重。OpenCode 同时读取数据库和旧文件，同一消息优先使用数据库内容，兼容新表替代旧表；fork 中早于子会话创建时间的复制记录不再计费。
- DeepSeek Harness 按 turn/step/request attempt 合并用量；后续样本替换同请求的先前值（包括向下修正），重试独立计入。仅统计原生 `tool/call`，流式工具片段不重复计数。v0–v1 的 `seedLength` 与 v2–v4 的带 `inherited` 标记边界排除复制的父历史，普通恢复标记保留历史用量。
- 输入总量 = 新增输入 + 缓存读取 + 缓存写入；输出包含可识别的推理 Token，推理是输出的子项，不重复加入总量。缓存命中率按缓存读取量除以对应输入总量计算。2.3.1 已导入且缺少缓存明细的旧代理摘要继续计入 Token 总量，并排除在缓存命中率之外。
- 并行会话的活跃时长累加，表示累计 agent 工作量。Codex 沿用生命周期与等待扣除规则；OpenCode 使用已完成消息的原生起止时间，同一会话内重叠区间只计一次；DeepSeek Harness 使用 step 起止时间并保留工具执行时间，原生 turn 与首个流式 token 时间用于首字延迟；没有原生区间时沿用可配置空闲间隔估算并明确标记。Claude Code 的消息间隔估算不能准确识别全部用户等待，不等同于人工投入时间。
- 首字延迟只用原生计时，无数据返回空值；不会从消息时间差补造首字延迟。没有活跃计时则 TPS 为空。

## 成本与价格

同一模型在所有 agent 和 API 路由使用同一供应商的官方 API 参考价，单位为 USD / 百万 Token。自定义价格按模型生效。完整型号、档位与官方来源见 [官方价格说明](pricing.md)。

`hy4free` 等平台路由标签统一为基础模型名，Flash、Preview 等独立型号仍分别统计。即使来源平台缺失，日志中有明确型号的用量也可按模型查询。缺少型号的历史用量保留在总量和会话里，价格缺失的型号按原名显示为未定价。

DeepSeek 根据请求 UTC 时间选择峰谷价。部分 Anthropic、Gemini、MiniMax、Qwen、Grok 型号根据单次请求输入选择上下文档位。已记录的计价档位随永久摘要保留，旧摘要缺少请求档位时按标准价估算。Anthropic 和 Kimi 的原生 1 小时缓存写入按输入价 2 倍计算。

日志自带成本保存在独立 `reportedCostMicrousd` 字段。当前仅“检查 OpenAI 更新”执行用户主动触发的联网价格检查，其他品牌随内置快照交付。

## 同步、保留与兼容边界

Codex 沿用安全字节偏移追加。Claude Code 对变化的会话文件重放结构化快照，以正确合并后续补全的流式消息；OpenCode 按会话行数、更新时间、大小及旧文件身份检测变化，对变化的会话重新抽取，支持 WAL 中尚未 checkpoint 的更新。没有变化的会话不会重新入库。新适配器的资源使用尚未做真实大规模日志 benchmark，不能直接沿用 Codex 的吞吐结论。

重放在单来源事务中替换派生记录；取消或读取失败会回滚本来源，保留前次快照。DeepSeek Harness 不解包落盘、不修复或修改源文件；未完成的尾部保留完整记录并标注部分数据，完整行或压缩校验损坏时回滚，未知版本明确报错。某个 agent 的来源检测失败会报告脱敏错误码，其他 agent 继续同步。缺失的源文件保留已有历史；清空和重新索引仍沿用原有数据维护含义。分析库的事件明细保留 90 天，永久摘要不依赖源文件仍存在。

Claude Code 默认可能清理历史日志，OpenCode 的内部结构也会升级；只能导入磁盘仍可访问且受支持的记录，不能恢复已经被源工具删除的历史。当前通过合成 JSONL、旧 JSON、多代 SQLite/WAL 与迁移用例验证。2026-10-26 的本机验收额外发现并补齐 OpenCode 2.1.3/2.1.4 的 `session_v2` 结构；真实数据库的 8 个会话在隔离分析库中成功导入，再次同步无变化，未修改源数据库。Claude Code 真实日志、macOS 真机及大规模性能仍未验收。

格式和去重设计参考 [ccusage 的 OpenCode adapter](https://github.com/ccusage/ccusage/tree/main/rust/adapters/opencode/src)、[Tokscale 的 Claude Code parser](https://github.com/junhoyeo/tokscale/blob/main/crates/tokscale-core/src/sessions/claudecode.rs) 以及 [OpenCode v1.19.4 源码](https://github.com/anomalyco/opencode/blob/v1.19.4/packages/core/src/session/sql.ts)。Claude 的存储位置和保留行为参见 [官方会话文档](https://code.claude.com/docs/en/sessions)。

DeepSeek Harness 格式与去重口径依据[官方 session persistence](https://github.com/deepseek-ai/deepseek-harness/tree/master/packages/session/session-persistence-jsonl)、[用量投影](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/llm/token-meter/src/usage-projection.ts)与[格式版本记录](https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/session-format-status.md)。原生 v0 与当前 v4 的格式分别核对本机源码和官方当前源码，测试覆盖原始/压缩文件、代际选择、继承与恢复、用量修正、重试和版本拒绝。

2026-10-26 本机 DeepSeek Harness v0 验收：7 个会话、4,109,674 Token、70 次模型请求、112 次工具调用，与独立 Zstandard/JSON 复算逐会话一致；首次和重复同步无读取/解析错误，重复同步无变化，源文件 SHA-256 不变。v1–v4 仍以合成回归验证，未声称真实新版本或 macOS 真机验收。

正式安装后的分析库另以当前目录独立复核 8 个逻辑会话（含新增空会话），同时覆盖真实 v0/v4 文件；总量仍为 4,109,674 Token、70 次请求、112 次工具调用，最新代际选择保持统计一致，旧源文件哈希保持不变。


## WorkBuddy、WorkBuddy AI 与 CodeBuddy Code CLI

两个桌面版本和 CodeBuddy Code CLI 归为同一个 `workbuddy` agent。CLI 目录按显式配置、`CODEBUDDY_CONFIG_DIR`、`~/.codebuddy` 的顺序选择。三者从 `projects/**/*.jsonl` 读取用量，桌面版使用 `workbuddy.db` 补充工作区、创建时间和归档状态。只读取统计所需字段。[CLI 目录说明](https://www.workbuddy.cn/docs/cli/codebuddy-dir)

`providerData.usage` 优先，其次 `rawUsage`，最后使用兼容的 `message.usage`；同一调用的多种表示不累加。输入总量已经包含缓存读写，新增输入扣除两者；推理是输出子项。原生消息标识去重调用，调用 ID 去重工具，复制到不同来源的同一会话合并统计。明确失败、取消、中断或不完整的记录跳过；同一会话此前完成的调用保留。缺失 Token 不按文本估算。

trace 与 transcript 当前不能可靠关联，因此 trace 汇总和耗时不参与统计，WorkBuddy 不启用空闲间隔估时。缺失计时的活跃时长、TPS、TTFT 显示 `—`；接口返回 `activeTimingAvailable: false`，CSV 导出为空，JSON 导出对应计时为 `null`。混合统计只计已有时长，模型 TPS 不纳入无计时的输出。

模型按记录中的原生 ID 归并后匹配官方参考价与用户覆盖。已识别的 `hy4-preview-f` 与 `hy4free` 分别归为 `hy4-preview` 与 `hy4`。工作区归并与其他 agent 一致，会话 ID 使用 `workbuddy:` 命名空间。当前真实样本来自 Windows WorkBuddy 5.5.6 和 WorkBuddy AI 5.9.4；macOS 仅由跨平台测试与构建验证，不等同于真机日志验收。

2026-11-06 Windows 本机隔离验收：两版共 29 个会话、976 次原生调用、89,459,124 Token，与独立按原始 usage 复算的输入、缓存读写、输出和推理量逐型号一致。29 个来源首次同步无解析/读取错误，再次同步无变化；29 份 transcript 的 SHA-256 不变。原生活跃片段为零，未以消息间隔推算时长。

CLI 需要启用原生会话保存。使用 `--no-session-persistence` 的调用不会生成可导入的会话 JSONL。[CLI 参数说明](https://www.workbuddy.cn/docs/cli/cli-reference)

2.3.2 起统一从上述三个原生目录接入。升级时忽略旧版的 `workbuddyProxy` 路径设置，已经导入的统计按永久摘要保留。

## Z Code

默认读取 `~/.zcode/cli/db/db.sqlite`，显式配置优先于 `ZCODE_HOME` 和默认目录；也可直接选择数据库文件的绝对路径。`ZCODE_HOME` 在 Chronolume 中表示单个数据根目录。只读连接保留源 WAL 模式，并在读取事务内提取统计快照；不读取配置、凭据、对话、原始 usage JSON 或错误正文。

Token 以 `model_usage` 为唯一来源，每个原生请求 ID 计入一次，重试分别计入；不再累加 `turn_usage` 汇总或消息中的重复用量。保留 `completed`、`error`、`cancelled` 请求已经报告的用量，`running` 等待请求结束后同步。输入已包含缓存读取和写入，因此新增输入扣除两者；推理属于输出子项。没有记录的 Token 不按文本估算。成本继续按模型使用项目已有的官方参考价和用户自定义价，不代表 Coding Plan 实际账单。

工具按 `tool_usage.tool_call_id` 去重，包含已结束的失败与取消调用。活跃时间取请求和工具原生起止区间的并集，不累计重叠区间，也不按请求间隔补造时间；没有结束时间时可使用原生 `duration_ms`。首字延迟使用 `turn_usage` 的原生首次响应计时，每轮一个样本，按该轮首个实际响应的请求归属模型。缺少计时显示 `—`。会话使用 `zcode:` 命名空间，保留父子关系；归档或软删除会话归为归档历史。

来源变化检测对所选结构化字段生成指纹，可以发现未 checkpoint 的 WAL 更新，以及记录数、字段长度和会话更新时间均不变的用量修正。缺失的源会话保留已导入历史；结构不支持时报告脱敏错误码，其他 agent 继续同步。当前兼容具有原生请求用量表的格式；缺少可选缓存、工具或轮次表的旧格式保留可读统计，不从会话正文回填。

字段和统计口径核对了 [Z Code 官方用量存储实现](https://github.com/zai-org/ZCode/blob/a9fe401eede6e6e5e01be9048679c206fe6f02d9/apps/zcode-cli/packages/adapters/src/storage/session-store/repositories/usage.ts)及[缓存输入口径](https://github.com/zai-org/ZCode/blob/a9fe401eede6e6e5e01be9048679c206fe6f02d9/apps/zcode-cli/packages/core/src/runtime/methods/turn-model-step-usage.ts)。仅接入本地应用用量；[官方“编程套餐”远端额度](https://zcode.z.ai/cn/docs/usage-stats)不在读取范围内。

2026-11-07 Windows 本机隔离验收：7 个会话、38 次已结束请求（含 4 次零 Token 失败请求）、2,988,045 Token、31 次工具调用及 3,655,554 毫秒原生活跃时间，与独立 SQL 和区间并集复算逐会话一致；2 个原生轮次首字延迟样本。再次同步无变化，源数据库和 WAL 的 SHA-256 不变。样本会话存储版本为 `0.16.5`、`0.17.1`；旧格式由合成回归覆盖，尚未做 macOS 真机或大规模日志性能验收。

## Google Antigravity

在数据页打开 **Google Antigravity → 配置目录**，可分别设置 Antigravity App、CLI 和 IDE 的数据根目录。三个客户端共用一个 agent 筛选项。每个会话优先读取 `brain/<会话 ID>/.system_generated/logs/transcript.jsonl`，缺失时读取同目录的 `transcript_full.jsonl`。[Google 官方目录说明](https://antigravity.google/docs/hooks)

会话 ID 与 `step_index` 共同标识一次模型步骤。跨客户端副本按文件修改时间从早到晚合并。同一步骤出现多次时取最后一条记录，支持向下修正。对 `PLANNER_RESPONSE` 分别统计 `input_tokens`（新增输入）、`cache_read_tokens`（缓存读取）和 `output_tokens`，总量为三者之和。工具按该步骤中每个具名调用分别计数。模型使用日志中的型号，参考成本按 Chronolume 的价格表计算。[Token 字段口径](https://github.com/vikrant82/agy-token-stats)

可选摘要库 `conversation_summaries.db` 用于关联父会话和工作区。缺少唯一可识别的本地工作区时，会话归入未知工作区。现有日志仅提供步骤时间戳，活跃时长、生成速度和首字延迟显示为 `—`。

2026-11-21 在 Windows 桌面版日志上逐会话独立复算，共 8 个会话、74 个带用量的模型步骤、6,016,622 Token、66 次工具调用。再次同步处理 0 个会话，16 份日志、摘要库及其辅助文件的 SHA-256 均保持一致。CLI 与 IDE 目录通过合成样本验证，macOS 尚未进行真机验收。
