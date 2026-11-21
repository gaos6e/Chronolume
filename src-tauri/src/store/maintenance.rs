use chrono::Utc;
use rusqlite::{OptionalExtension, params};
use serde::Serialize;
use uuid::Uuid;

use super::UsageStore;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSettingsRecord {
    pub id: String,
    pub alias: Option<String>,
    pub ignored: bool,
}

impl UsageStore {
    pub fn update_workspace_settings(
        &self,
        workspace_id: &str,
        alias: Option<&str>,
        ignored: bool,
    ) -> AppResult<WorkspaceSettingsRecord> {
        let alias = alias.map(str::trim).filter(|value| !value.is_empty());
        if alias.is_some_and(|value| value.chars().count() > 80) {
            return Err(AppError::new(
                "invalid_alias",
                "工作区别名不能超过 80 个字符",
            ));
        }
        self.with_writer(|transaction| {
            let updated = transaction.execute(
                "UPDATE workspaces SET alias = ?2, ignored = ?3, updated_at_ms = ?4 WHERE id = ?1",
                params![
                    workspace_id,
                    alias,
                    i64::from(ignored),
                    Utc::now().timestamp_millis()
                ],
            )?;
            if updated == 0 {
                return Err(AppError::new("workspace_not_found", "工作区不存在"));
            }
            Ok(WorkspaceSettingsRecord {
                id: workspace_id.to_string(),
                alias: alias.map(str::to_string),
                ignored,
            })
        })
    }

    pub fn clear_analysis(&self) -> AppResult<()> {
        self.clear_index_tables(true)
    }

    pub(crate) fn reset_index(&self) -> AppResult<()> {
        self.clear_index_tables(false)
    }

    fn clear_index_tables(&self, clear_sync_history: bool) -> AppResult<()> {
        // A live SQLite snapshot includes committed WAL data. A plain file copy
        // can silently omit the newest imported sessions.
        self.backup_database(if clear_sync_history {
            "clear"
        } else {
            "reindex"
        })?;
        self.with_writer(|transaction| {
            for table in [
                "daily_tool_rollups",
                "daily_usage_rollups",
                "session_daily_tool",
                "session_daily_usage",
                "session_turn_latencies",
                "tool_events",
                "usage_events",
                "activity_segments",
                "session_model_segments",
                "sessions",
                "source_files",
                "workspaces",
            ] {
                transaction.execute(&format!("DELETE FROM {table}"), [])?;
            }
            if clear_sync_history {
                transaction.execute("DELETE FROM sync_runs", [])?;
            }
            Ok(())
        })?;
        self.with_reader(|connection| {
            connection.execute_batch("PRAGMA wal_checkpoint(PASSIVE)")?;
            Ok(())
        })
    }

    fn backup_database(&self, reason: &str) -> AppResult<()> {
        let backup = || -> AppResult<()> {
            let directory = self
                .path
                .parent()
                .filter(|path| !path.as_os_str().is_empty())
                .ok_or_else(AppError::filesystem)?
                .join("backups");
            std::fs::create_dir_all(&directory)?;
            let path = directory.join(format!(
                "chronolume-{}-{reason}-{}.sqlite3",
                Utc::now().format("%Y%m%dT%H%M%S%.3fZ"),
                Uuid::new_v4()
            ));
            // Reserve a unique, empty target; VACUUM INTO never overwrites an
            // existing backup. Only a fully written and checked snapshot is kept.
            let file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            drop(file);
            let result = (|| -> AppResult<()> {
                self.with_reader(|connection| {
                    connection.execute("VACUUM main INTO ?1", [path.to_string_lossy()])?;
                    Ok(())
                })?;
                let snapshot = rusqlite::Connection::open_with_flags(
                    &path,
                    rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
                )?;
                let check: String =
                    snapshot.query_row("PRAGMA quick_check(1)", [], |row| row.get(0))?;
                if check != "ok" {
                    return Err(AppError::database());
                }
                Ok(())
            })();
            if result.is_err() {
                let _ = std::fs::remove_file(&path);
            }
            result
        };
        backup().map_err(|_| {
            AppError::new(
                "database_backup_failed",
                "数据库备份失败，未执行重新索引或清空",
            )
        })
    }

    pub fn app_setting(&self, key: &str) -> AppResult<Option<String>> {
        self.with_reader(|connection| {
            connection
                .query_row(
                    "SELECT value_json FROM app_settings WHERE key = ?1",
                    [key],
                    |row| row.get(0),
                )
                .optional()
                .map_err(AppError::from)
        })
    }

    pub fn save_app_setting(&self, key: &str, value_json: &str) -> AppResult<()> {
        if key.is_empty()
            || key.len() > 120
            || serde_json::from_str::<serde_json::Value>(value_json).is_err()
        {
            return Err(AppError::new("invalid_setting", "设置键或 JSON 值无效"));
        }
        self.with_writer(|transaction| {
            transaction.execute(
                "INSERT INTO app_settings (key, value_json, updated_at_ms) VALUES (?1, ?2, ?3)
                 ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json,
                     updated_at_ms = excluded.updated_at_ms",
                params![key, value_json, Utc::now().timestamp_millis()],
            )?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clearing_analysis_backs_up_wal_and_keeps_sources_prices_and_settings() {
        let source_directory = tempfile::tempdir().unwrap();
        let source_path = source_directory.path().join("rollout.jsonl");
        std::fs::write(&source_path, b"private source remains").unwrap();
        let database_directory = tempfile::tempdir().unwrap();
        let store = UsageStore::open(database_directory.path().join("usage.sqlite3")).unwrap();
        store.seed_builtin_prices().unwrap();
        store.save_app_setting("test-setting", "\"kept\"").unwrap();
        store
            .with_writer(|transaction| {
                transaction.execute(
                    "INSERT INTO workspaces (id, normalized_path, display_name, ignored, created_at_ms, updated_at_ms)
                     VALUES ('w', 'C:/repo', 'repo', 0, 0, 0)",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO sessions (
                        id, workspace_id, synthetic_title, model_provider, latest_model_raw,
                        primary_model_raw, integrity_status, parser_version, updated_at_ms
                     ) VALUES ('s', 'w', 'Session', 'openai', 'gpt-5', 'gpt-5', 'complete', 1, 0)",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        // Pin a reader at the old snapshot so the session insert remains in WAL.
        let reader = store.connection().unwrap();
        reader
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); BEGIN; SELECT COUNT(*) FROM sessions;")
            .unwrap();
        store
            .with_writer(|transaction| {
                transaction.execute("UPDATE sessions SET total_tokens=12345 WHERE id='s'", [])?;
                Ok(())
            })
            .unwrap();
        store.clear_analysis().unwrap();
        let backups: Vec<_> = std::fs::read_dir(database_directory.path().join("backups"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(backups.len(), 1);
        assert!(
            backups[0]
                .file_name()
                .unwrap()
                .to_string_lossy()
                .contains("-clear-")
        );
        let snapshot = rusqlite::Connection::open_with_flags(
            &backups[0],
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let tokens: i64 = snapshot
            .query_row(
                "SELECT total_tokens FROM sessions WHERE id='s'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(tokens, 12345);
        let setting: String = snapshot
            .query_row(
                "SELECT value_json FROM app_settings WHERE key='test-setting'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(setting, "\"kept\"");
        reader.execute_batch("ROLLBACK").unwrap();
        assert_eq!(
            std::fs::read(&source_path).unwrap(),
            b"private source remains"
        );
        store
            .with_reader(|connection| {
                let sessions: i64 =
                    connection.query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))?;
                let prices: i64 =
                    connection
                        .query_row("SELECT COUNT(*) FROM model_prices", [], |row| row.get(0))?;
                assert_eq!(sessions, 0);
                assert!(prices > 0);
                Ok(())
            })
            .unwrap();
        assert_eq!(
            store.app_setting("test-setting").unwrap().as_deref(),
            Some("\"kept\"")
        );
        store.clear_analysis().unwrap();
        assert_eq!(
            std::fs::read_dir(database_directory.path().join("backups"))
                .unwrap()
                .count(),
            2
        );
        assert_eq!(
            snapshot
                .query_row(
                    "SELECT total_tokens FROM sessions WHERE id='s'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            12345
        );
    }

    #[test]
    fn backup_failure_aborts_both_clear_and_reindex_without_deleting_records() {
        for clear in [true, false] {
            let directory = tempfile::tempdir().unwrap();
            let store = UsageStore::open(directory.path().join("usage.sqlite3")).unwrap();
            store.with_writer(|transaction| {
                transaction.execute("INSERT INTO workspaces(id,normalized_path,display_name,created_at_ms,updated_at_ms) VALUES ('w','C:/repo','repo',0,0)", [])?;
                transaction.execute("INSERT INTO sessions(id,workspace_id,synthetic_title,total_tokens,parser_version,updated_at_ms) VALUES ('s','w','Session',123,11,0)", [])?;
                Ok(())
            }).unwrap();
            // A file at the backup-directory path deterministically blocks backup.
            std::fs::write(directory.path().join("backups"), b"blocked").unwrap();
            let result = if clear {
                store.clear_analysis()
            } else {
                store.reset_index()
            };
            assert_eq!(result.unwrap_err().code, "database_backup_failed");
            store
                .with_reader(|connection| {
                    let tokens: i64 = connection.query_row(
                        "SELECT total_tokens FROM sessions WHERE id='s'",
                        [],
                        |row| row.get(0),
                    )?;
                    assert_eq!(tokens, 123);
                    Ok(())
                })
                .unwrap();
        }
    }
}
