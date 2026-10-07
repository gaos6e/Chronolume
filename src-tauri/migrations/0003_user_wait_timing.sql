-- Timing metadata only; question and answer content must never enter the database.
ALTER TABLE source_files ADD COLUMN user_wait_timing_json TEXT NOT NULL DEFAULT '{}';
