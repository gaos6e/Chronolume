-- Rebuild the path uniqueness constraint while keeping the existing child tables.
PRAGMA defer_foreign_keys = ON;
CREATE TEMP TABLE old_workspaces AS SELECT * FROM workspaces;
CREATE TEMP TABLE workspace_agents AS
    SELECT workspace_id, agent FROM sessions
    UNION SELECT workspace_id, agent FROM session_daily_usage
    UNION SELECT workspace_id, agent FROM session_daily_tool
    UNION SELECT workspace_id, agent FROM daily_usage_rollups
    UNION SELECT workspace_id, agent FROM daily_tool_rollups;
CREATE UNIQUE INDEX temp.idx_workspace_agents ON workspace_agents(workspace_id, agent);
-- Preserve settings for paths with no remaining evidence of their source.
INSERT INTO workspace_agents
SELECT id, 'codex' FROM old_workspaces w
WHERE NOT EXISTS (SELECT 1 FROM workspace_agents WHERE workspace_id = w.id);

DROP TABLE workspaces;
CREATE TABLE workspaces (
    id TEXT PRIMARY KEY,
    agent TEXT NOT NULL DEFAULT 'codex',
    normalized_path TEXT NOT NULL,
    display_name TEXT NOT NULL,
    alias TEXT,
    ignored INTEGER NOT NULL DEFAULT 0 CHECK (ignored IN (0, 1)),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    UNIQUE (agent, normalized_path)
) STRICT;
CREATE INDEX idx_workspaces_activity ON workspaces(ignored, updated_at_ms DESC);
CREATE VIEW named_workspaces AS
    SELECT *, workspace_label(agent, COALESCE(alias, display_name)) AS label FROM workspaces;
INSERT INTO workspaces
SELECT CASE WHEN a.agent = 'codex' THEN w.id ELSE a.agent || ':' || w.id END,
       a.agent, w.normalized_path, workspace_basename(w.normalized_path), w.alias,
       w.ignored, w.created_at_ms, w.updated_at_ms
FROM old_workspaces w JOIN workspace_agents a ON a.workspace_id = w.id;

UPDATE sessions SET workspace_id = agent || ':' || workspace_id WHERE agent <> 'codex';
UPDATE session_daily_usage SET workspace_id = agent || ':' || workspace_id WHERE agent <> 'codex';
UPDATE session_daily_tool SET workspace_id = agent || ':' || workspace_id WHERE agent <> 'codex';
UPDATE daily_usage_rollups SET workspace_id = agent || ':' || workspace_id WHERE agent <> 'codex';
UPDATE daily_tool_rollups SET workspace_id = agent || ':' || workspace_id WHERE agent <> 'codex';

UPDATE app_settings SET value_json = json_set(value_json, '$.visibleWorkspaceIds', (
    SELECT json_group_array(id) FROM (
        SELECT DISTINCT CASE WHEN a.agent IS NULL OR a.agent = 'codex' THEN selected.value
                             ELSE a.agent || ':' || selected.value END AS id
        FROM json_each(value_json, '$.visibleWorkspaceIds') selected
        LEFT JOIN workspace_agents a ON a.workspace_id = selected.value
        ORDER BY id
    )
)) WHERE key = 'activity' AND json_valid(value_json)
   AND json_type(value_json, '$.visibleWorkspaceIds') = 'array';
DROP TABLE temp.workspace_agents;
DROP TABLE temp.old_workspaces;
