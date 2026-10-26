-- Agent identity follows the stable session namespace; existing IDs remain Codex.
ALTER TABLE sessions ADD COLUMN agent TEXT GENERATED ALWAYS AS (CASE WHEN id LIKE 'claude:%' THEN 'claude' WHEN id LIKE 'opencode:%' THEN 'opencode' ELSE 'codex' END) VIRTUAL;
ALTER TABLE usage_events ADD COLUMN agent TEXT GENERATED ALWAYS AS (CASE WHEN session_id LIKE 'claude:%' THEN 'claude' WHEN session_id LIKE 'opencode:%' THEN 'opencode' ELSE 'codex' END) VIRTUAL;
ALTER TABLE tool_events ADD COLUMN agent TEXT GENERATED ALWAYS AS (CASE WHEN session_id LIKE 'claude:%' THEN 'claude' WHEN session_id LIKE 'opencode:%' THEN 'opencode' ELSE 'codex' END) VIRTUAL;
ALTER TABLE session_daily_usage ADD COLUMN agent TEXT GENERATED ALWAYS AS (CASE WHEN session_id LIKE 'claude:%' THEN 'claude' WHEN session_id LIKE 'opencode:%' THEN 'opencode' ELSE 'codex' END) VIRTUAL;
ALTER TABLE session_daily_tool ADD COLUMN agent TEXT GENERATED ALWAYS AS (CASE WHEN session_id LIKE 'claude:%' THEN 'claude' WHEN session_id LIKE 'opencode:%' THEN 'opencode' ELSE 'codex' END) VIRTUAL;
ALTER TABLE session_turn_latencies ADD COLUMN agent TEXT GENERATED ALWAYS AS (CASE WHEN session_id LIKE 'claude:%' THEN 'claude' WHEN session_id LIKE 'opencode:%' THEN 'opencode' ELSE 'codex' END) VIRTUAL;
ALTER TABLE source_files ADD COLUMN agent TEXT GENERATED ALWAYS AS (CASE WHEN source_key LIKE 'claude:%' THEN 'claude' WHEN source_key LIKE 'opencode:%' THEN 'opencode' ELSE 'codex' END) VIRTUAL;
ALTER TABLE sessions ADD COLUMN parent_session_id TEXT;
ALTER TABLE usage_events ADD COLUMN cache_write_tokens INTEGER NOT NULL DEFAULT 0 CHECK (cache_write_tokens >= 0);
ALTER TABLE usage_events ADD COLUMN cache_write_1h_tokens INTEGER NOT NULL DEFAULT 0 CHECK (cache_write_1h_tokens >= 0 AND cache_write_1h_tokens <= cache_write_tokens);
ALTER TABLE usage_events ADD COLUMN reported_cost_microusd INTEGER CHECK (reported_cost_microusd >= 0);
ALTER TABLE sessions ADD COLUMN cache_write_tokens INTEGER NOT NULL DEFAULT 0 CHECK (cache_write_tokens >= 0);
ALTER TABLE sessions ADD COLUMN cache_write_1h_tokens INTEGER NOT NULL DEFAULT 0 CHECK (cache_write_1h_tokens >= 0 AND cache_write_1h_tokens <= cache_write_tokens);
ALTER TABLE sessions ADD COLUMN reported_cost_microusd INTEGER CHECK (reported_cost_microusd >= 0);
ALTER TABLE session_daily_usage ADD COLUMN cache_write_tokens INTEGER NOT NULL DEFAULT 0 CHECK (cache_write_tokens >= 0);
ALTER TABLE session_daily_usage ADD COLUMN cache_write_1h_tokens INTEGER NOT NULL DEFAULT 0 CHECK (cache_write_1h_tokens >= 0 AND cache_write_1h_tokens <= cache_write_tokens);
ALTER TABLE session_daily_usage ADD COLUMN reported_cost_microusd INTEGER CHECK (reported_cost_microusd >= 0);
ALTER TABLE session_model_segments ADD COLUMN cache_write_tokens INTEGER NOT NULL DEFAULT 0 CHECK (cache_write_tokens >= 0);
ALTER TABLE session_model_segments ADD COLUMN cache_write_1h_tokens INTEGER NOT NULL DEFAULT 0 CHECK (cache_write_1h_tokens >= 0 AND cache_write_1h_tokens <= cache_write_tokens);
ALTER TABLE session_model_segments ADD COLUMN reported_cost_microusd INTEGER CHECK (reported_cost_microusd >= 0);
ALTER TABLE daily_usage_rollups RENAME TO daily_usage_rollups_v4;
CREATE TABLE daily_usage_rollups (
    local_date TEXT NOT NULL,
    timezone_id TEXT NOT NULL,
    day_start_utc_ms INTEGER NOT NULL,
    day_end_utc_ms INTEGER NOT NULL,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    model_provider TEXT NOT NULL,
    model_raw TEXT NOT NULL,
    pricing_model_id TEXT NOT NULL DEFAULT '',
    archived INTEGER NOT NULL CHECK (archived IN (0, 1)),
    session_count INTEGER NOT NULL DEFAULT 0 CHECK (session_count >= 0),
    active_ms INTEGER NOT NULL DEFAULT 0 CHECK (active_ms >= 0),
    input_tokens INTEGER NOT NULL DEFAULT 0 CHECK (input_tokens >= 0),
    fresh_input_tokens INTEGER NOT NULL DEFAULT 0 CHECK (fresh_input_tokens >= 0),
    cached_input_tokens INTEGER NOT NULL DEFAULT 0 CHECK (cached_input_tokens >= 0),
    output_tokens INTEGER NOT NULL DEFAULT 0 CHECK (output_tokens >= 0),
    reasoning_tokens INTEGER NOT NULL DEFAULT 0 CHECK (reasoning_tokens >= 0),
    total_tokens INTEGER NOT NULL DEFAULT 0 CHECK (total_tokens >= 0),
    priced_cost_microusd INTEGER NOT NULL DEFAULT 0 CHECK (priced_cost_microusd >= 0),
    unpriced_event_count INTEGER NOT NULL DEFAULT 0 CHECK (unpriced_event_count >= 0),
    last_activity_at_ms INTEGER,
    cache_write_tokens INTEGER NOT NULL DEFAULT 0,
    cache_write_1h_tokens INTEGER NOT NULL DEFAULT 0,
    reported_cost_microusd INTEGER,
    agent TEXT NOT NULL DEFAULT 'codex' CHECK (agent IN ('codex', 'claude', 'opencode')),
    PRIMARY KEY (agent, local_date, timezone_id, workspace_id, model_provider, model_raw, pricing_model_id, archived)
) STRICT, WITHOUT ROWID;
INSERT INTO daily_usage_rollups (local_date, timezone_id, day_start_utc_ms, day_end_utc_ms, workspace_id, model_provider, model_raw, pricing_model_id, archived, session_count, active_ms, input_tokens, fresh_input_tokens, cached_input_tokens, output_tokens, reasoning_tokens, total_tokens, priced_cost_microusd, unpriced_event_count, last_activity_at_ms) SELECT local_date, timezone_id, day_start_utc_ms, day_end_utc_ms, workspace_id, model_provider, model_raw, pricing_model_id, archived, session_count, active_ms, input_tokens, fresh_input_tokens, cached_input_tokens, output_tokens, reasoning_tokens, total_tokens, priced_cost_microusd, unpriced_event_count, last_activity_at_ms FROM daily_usage_rollups_v4;
DROP TABLE daily_usage_rollups_v4;
ALTER TABLE daily_tool_rollups RENAME TO daily_tool_rollups_v4;
CREATE TABLE daily_tool_rollups (
    local_date TEXT NOT NULL,
    timezone_id TEXT NOT NULL,
    day_start_utc_ms INTEGER NOT NULL,
    day_end_utc_ms INTEGER NOT NULL,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    archived INTEGER NOT NULL CHECK (archived IN (0, 1)),
    tool_name TEXT NOT NULL,
    category TEXT NOT NULL CHECK (category IN ('search', 'read', 'write', 'edit', 'execute', 'other')),
    operation_kind TEXT NOT NULL CHECK (operation_kind IN ('read_only', 'mutating', 'mixed', 'unknown')),
    call_count INTEGER NOT NULL DEFAULT 0 CHECK (call_count >= 0),
    session_count INTEGER NOT NULL DEFAULT 0 CHECK (session_count >= 0),
    agent TEXT NOT NULL DEFAULT 'codex' CHECK (agent IN ('codex', 'claude', 'opencode')),
    PRIMARY KEY (agent, local_date, timezone_id, workspace_id, archived, tool_name, category, operation_kind)
) STRICT, WITHOUT ROWID;
INSERT INTO daily_tool_rollups (local_date, timezone_id, day_start_utc_ms, day_end_utc_ms, workspace_id, archived, tool_name, category, operation_kind, call_count, session_count) SELECT local_date, timezone_id, day_start_utc_ms, day_end_utc_ms, workspace_id, archived, tool_name, category, operation_kind, call_count, session_count FROM daily_tool_rollups_v4;
DROP TABLE daily_tool_rollups_v4;
CREATE INDEX idx_sessions_agent_time ON sessions(agent, started_at_ms DESC);
CREATE INDEX idx_daily_usage_agent_filter ON daily_usage_rollups(agent, workspace_id, model_provider, model_raw, local_date);
CREATE INDEX idx_daily_tool_agent_filter ON daily_tool_rollups(agent, workspace_id, local_date);
