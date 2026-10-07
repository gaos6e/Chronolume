# Chronolume 2.1 数据模型

v2 分析数据库在 Windows 位于 `%LOCALAPPDATA%\Chronolume\v2\chronolume-v2.sqlite3`，在 macOS 位于 `~/Library/Application Support/Chronolume/v2/chronolume-v2.sqlite3`。数据库仅存结构化统计和恢复索引所需元数据，不存会话正文、工具参数、命令或代码。Windows 保留 2.0.1 的旧品牌数据库/WAL sidecar 迁移；macOS 从不探测或迁移 Windows 历史目录。

工作区显示路径保留 Codex 记录中的可读形式。Windows 继续使用原有斜杠规范化和 ASCII 大小写不敏感哈希，避免已有 ID 改变；macOS 对已存在路径只读 `canonicalize`，由实际卷决定大小写语义并合并符号链接，无法规范化或不存在的路径按原大小写保守区分。该过程不会向工作区写入探测文件。

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
| `session_daily_usage` | 永久 | 会话/日期/模型中间汇总，支持精准筛选和改价 |
| `session_daily_tool` | 永久 | 会话/日期/工具类别中间汇总 |
| `daily_usage_rollups` | 永久 | 工作区/提供方/模型/日聚合 |
| `daily_tool_rollups` | 永久 | 工具类别/工具名/日聚合 |
| `model_prices` | 永久 | 内置价格和用户覆盖 |
| `app_settings` | 永久 | 非敏感应用设置 |

`usage_events` 和 `tool_events` 是唯一用于事件分页的表，按本地日边界删除 90 天前记录；删除前先重建永久汇总。`session_model_segments` 与 `activity_segments` 属于会话摘要，只保存模型/时间/计数，不保存消息、工具参数、命令或文件路径。

TPS 使用输出 Token（已包含推理 Token）除以活跃秒数。会话 `tokensPerSecond` 从完整会话摘要计算；模型 `averageTokensPerSecond` 从当前筛选范围内 `session_daily_usage` 的总输出 Token 和总活跃时长计算，按时长加权。这些字段在查询时计算，事件明细过期后仍可使用永久汇总。活跃时长包含工具调用等等待，沿用现有生命周期及估算口径；总活跃时长为零时返回 `null`，有时长但无输出时返回 `0`。

活跃时长排除可识别的用户答复等待：同步 `request_user_input` 从提问到答复或任务中断之间扣时；普通模式的异步询问不直接扣除提问到答复的整个区间，只在问题待答期间出现明确的 `clock.sleep` 暂停时扣除暂停。同期有工具执行或结构化模型工作记录的区间保留，计划分析、编写和其他工具等待照常计时。仅凭事件间隔长或没有新输出不推断用户等待。

等待区间按实际时间从活跃区间中切除后再分配到每日汇总，避免跨日等待按比例分摊到工作日。缺少生命周期的旧日志仍沿用空闲间隔估算，但会先扣掉可识别等待。`0003_user_wait_timing.sql` 自动增加来源计时检查点，保存时间区间及未完成调用 ID，不保存问题、答案、推理或工具原文；增量同步、应用重启和汇总重建均复用该状态。解析器升级后自动重放仍可访问的原始日志并更新历史统计，无需用户清库；原始日志已不可用的历史数据不能补推缺失的等待信息。

`source_files` 保存来源相对路径、类型、大小、mtime 纳秒、前缀哈希、安全偏移、完整行偏移、日志 rowid 水位、当前模型/提供方、累计 Token、解析器版本和脱敏错误码。所有表使用严格约束、外键和面向时间/筛选/分页的索引；完整定义以 [0001_initial.sql](../src-tauri/migrations/0001_initial.sql) 为准。
