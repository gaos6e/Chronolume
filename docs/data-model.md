# Chronolume 2.1 数据模型

v2 分析数据库在 Windows 位于 `%LOCALAPPDATA%\Chronolume\v2\chronolume-v2.sqlite3`，在 macOS 位于 `~/Library/Application Support/Chronolume/v2/chronolume-v2.sqlite3`。数据库仅存结构化统计和恢复索引所需元数据，不存会话正文、工具参数、命令或代码。Windows 保留 2.0.1 的旧品牌数据库/WAL sidecar 迁移；macOS 从不探测或迁移 Windows 历史目录。

工作区显示路径保留 各 agent 记录中的可读形式。Windows 继续使用原有斜杠规范化和 ASCII 大小写不敏感哈希，避免已有 ID 改变；macOS 对已存在路径只读 `canonicalize`，由实际卷决定大小写语义并合并符号链接，无法规范化或不存在的路径按原大小写保守区分。该过程不会向工作区写入探测文件。

| 表 | 保留期 | 主要职责 |
| --- | --- | --- |
| `schema_migrations` | 永久 | 事务性 Schema 版本 |
| `source_files` | 永久 | 安全字节偏移、解析状态和文件身份 |
| `sync_runs` | 永久/可压缩 | 导入进度、性能和错误计数 |
| `workspaces` | 永久 | 规范化、别名和忽略状态 |
| `sessions` | 永久 | 会话级 Token、成本、活跃时间和完整性摘要 |
| `usage_events` | 90 天 | Token delta 明细 |
| `tool_events` | 90 天 | 已脱敏工具名与类别明细 |
| `session_model_segments` | 永久 | 会话级模型切换摘要，不含正文 |
| `activity_segments` | 永久 | 会话级生命周期/估算摘要，不含事件参数 |
| `session_turn_latencies` | 永久 | 每轮原生首字延迟、模型与时间，不含正文 |
| `session_daily_usage` | 永久 | 会话/日期/模型中间汇总，支持精准筛选和改价 |
| `session_daily_tool` | 永久 | 会话/日期/工具类别中间汇总 |
| `daily_usage_rollups` | 永久 | 工作区/提供方/模型/日聚合 |
| `daily_tool_rollups` | 永久 | 工具类别/工具名/日聚合 |
| `model_prices` | 永久 | 内置价格和用户覆盖 |
| `app_settings` | 永久 | 非敏感应用设置 |

Schema 5 增加独立 agent 维度、父会话关系、缓存写入 Token（含原生 1 小时子项）和日志报告成本。既有 Codex ID、工作区别名、用户价格和设置不变；每日全局汇总按 agent 分组。Schema 6 扩展 DeepSeek Harness 的 agent 生成列及全局汇总约束，保留既有三个来源的会话、工具和缓存摘要。新 adapter 的识别与事务重放见 [agent 接入说明](agents.md)。

`usage_events` 和 `tool_events` 是唯一用于事件分页的表，按本地日边界删除 90 天前记录；删除前先重建永久汇总。`session_model_segments` 与 `activity_segments` 属于会话摘要，只保存模型/时间/计数，不保存消息、工具参数、命令或文件路径。

TPS 使用输出 Token（已包含推理 Token）除以活跃秒数。会话 `tokensPerSecond` 从完整会话摘要计算；模型 `averageTokensPerSecond` 仅使用当前筛选范围内 `session_daily_usage` 中 `active_ms > 0` 的记录，将其总输出 Token 除以总活跃时长，按时长加权。没有计时的记录仍保留在 Token、成本和会话数统计中，但不增加 TPS 的分子；TPS 排序使用相同口径。这些字段在查询时计算，事件明细过期后仍可使用永久汇总。活跃时长包含工具调用等等待，沿用现有生命周期及估算口径；总活跃时长为零时返回 `null`，有时长但无输出时返回 `0`。

平均首字延迟（TTFT）读取轮次结束事件中的原生 `time_to_first_token_ms`，以有效轮次的延迟总和除以轮次数计算，不按会话平均值再次取平均。`SessionRow` 和 `ModelRow` 都返回 `averageFirstTokenLatencyMs`（毫秒，缺失为 `null`）及 `firstTokenLatencySampleCount`（无样本为 `0`）。会话列表与详情使用整个会话的有效轮次；模型使用当前日期、工作区、提供方、模型及归档筛选范围内的轮次，同名模型沿用现有合并规则。日期取结束事件日志时间对应的本地日期；模型取该轮首次生效的模型上下文。仅接受 SQLite 整数范围内的非负整数，真实 `0` 计入，缺失或非法值不计入，也不从消息间隔估算；带有效原生计时的完成或中断事件均可计入。

`0004_first_token_latency.sql` 新增永久 `session_turn_latencies` 表及来源模型上下文检查点。轮次按会话与 `turn_id` 去重，缺少 `turn_id` 时以来源文件与字节偏移识别；追加、重启、来源重放和归档迁移保持一致，90 天事件清理不会删除这些结构化计时。Schema 版本 4、解析器版本 11 会自动重放仍可访问的原始日志以补齐历史；日志没有原生计时字段或已不可访问时无法补齐。CSV 在末尾追加平均延迟毫秒与样本数列，JSON 使用同名 camelCase 字段。

已开始但未结束的轮次，按任务开始到最新已观察日志事件的区间补入活跃时长，扣除可识别的用户等待，并标记为 `span_estimate`。没有新日志时不会按当前墙钟时间外推。该估算随增量日志替换，轮次结束后由正式生命周期计时替换，不与旧快照或空闲间隔估算重复累加；会话只要仍包含估算区间，`activeIsEstimate` 就为 `true`。检查点保存的开始时间及脱敏计时状态支持重启和派生汇总重建；解析器版本 10 会自动重放仍可访问的旧日志以修正进行中轮次的统计。

活跃时长排除可识别的用户答复等待：同步 `request_user_input` 从提问到答复或任务中断之间扣时；普通模式的异步询问不直接扣除提问到答复的整个区间，只在问题待答期间出现明确的 `clock.sleep` 暂停时扣除暂停。同期有工具执行或结构化模型工作记录的区间保留，计划分析、编写和其他工具等待照常计时。仅凭事件间隔长或没有新输出不推断用户等待。

等待区间按实际时间从活跃区间中切除后再分配到每日汇总，避免跨日等待按比例分摊到工作日。缺少生命周期的旧日志仍沿用空闲间隔估算，但会先扣掉可识别等待。`0003_user_wait_timing.sql` 自动增加来源计时检查点，保存时间区间及未完成调用 ID，不保存问题、答案、推理或工具原文；增量同步、应用重启和汇总重建均复用该状态。解析器升级后自动重放仍可访问的原始日志并更新历史统计，无需用户清库；原始日志已不可用的历史数据不能补推缺失的等待信息。

`source_files` 保存来源相对路径、类型、大小、mtime 纳秒、前缀哈希、安全偏移、完整行偏移、日志 rowid 水位、当前模型/提供方、累计 Token、解析器版本和脱敏错误码。所有表使用严格约束、外键和面向时间/筛选/分页的索引；完整定义以 [0001_initial.sql](../src-tauri/migrations/0001_initial.sql) 为准。

Schema 7 将已确认的同一模型历史别名统一为 `gpt-6.1-sol`。迁移合并发生主键冲突的永久摘要，保留数量、成本、计时和缓存计数；会话数基于永久会话 ID 去重，已过期明细无需重读源日志。来源水位、首字延迟上下文和模型价格身份同步规范化，新入库记录经过同一规则。
