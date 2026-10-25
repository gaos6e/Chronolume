-- Structured timing only; no response content or tool arguments.
ALTER TABLE source_files ADD COLUMN first_token_context_json TEXT NOT NULL DEFAULT '{}';

CREATE TABLE session_turn_latencies (
    id INTEGER PRIMARY KEY,
    source_file_id INTEGER NOT NULL REFERENCES source_files(id) ON DELETE CASCADE,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    byte_offset INTEGER NOT NULL CHECK (byte_offset >= 0),
    turn_id TEXT,
    occurred_at_ms INTEGER NOT NULL,
    local_date TEXT NOT NULL,
    timezone_id TEXT NOT NULL,
    model_provider TEXT NOT NULL,
    model_raw TEXT NOT NULL,
    latency_ms INTEGER NOT NULL CHECK (latency_ms >= 0),
    UNIQUE (source_file_id, byte_offset)
) STRICT;

CREATE UNIQUE INDEX idx_turn_latency_identity ON session_turn_latencies(session_id, turn_id)
    WHERE turn_id IS NOT NULL;
CREATE INDEX idx_turn_latency_session ON session_turn_latencies(session_id, model_raw);
CREATE INDEX idx_turn_latency_date_model ON session_turn_latencies(local_date, model_raw, model_provider);
