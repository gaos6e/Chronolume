mod ingest;
mod maintenance;
mod pricing_store;
mod sync_runs;

pub use ingest::{FileApplyResult, RetentionResult, retention_cutoff_utc_ms};
pub use maintenance::WorkspaceSettingsRecord;
pub use pricing_store::{ModelPriceInput, ModelPriceRecord, RepriceResult};
pub use sync_runs::SyncRunProgress;

use std::path::{Path, PathBuf};
use std::time::Duration;

use r2d2::{Pool, PooledConnection};
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::functions::FunctionFlags;
use rusqlite::{Connection, OpenFlags, Transaction, TransactionBehavior};

use crate::error::{AppError, AppResult};
use crate::pricing::{canonical_model_name, model_strength_sort_key};

const MIGRATIONS: &[(i64, &str)] = &[
    // Data-only migrations also run for databases whose source events expired.
    (1, include_str!("../../migrations/0001_initial.sql")),
    (
        2,
        include_str!("../../migrations/0002_embedded_transcript.sql"),
    ),
    (
        3,
        include_str!("../../migrations/0003_user_wait_timing.sql"),
    ),
    (
        4,
        include_str!("../../migrations/0004_first_token_latency.sql"),
    ),
    (5, include_str!("../../migrations/0005_multi_agent.sql")),
    (
        6,
        include_str!("../../migrations/0006_deepseek_harness.sql"),
    ),
    (
        7,
        include_str!("../../migrations/0007_canonical_model_identity.sql"),
    ),
];

/// v2 分析库的深 Module。连接配置、迁移、池化和事务策略留在实现内部。
#[derive(Clone)]
pub struct UsageStore {
    pool: Pool<SqliteConnectionManager>,
    path: PathBuf,
}

impl UsageStore {
    pub fn open(path: impl AsRef<Path>) -> AppResult<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let manager = SqliteConnectionManager::file(&path)
            .with_flags(
                OpenFlags::SQLITE_OPEN_READ_WRITE
                    | OpenFlags::SQLITE_OPEN_CREATE
                    | OpenFlags::SQLITE_OPEN_FULL_MUTEX,
            )
            .with_init(configure_connection);
        let pool = Pool::builder()
            .max_size(6)
            .min_idle(Some(1))
            .connection_timeout(Duration::from_secs(5))
            .build(manager)?;

        let store = Self { pool, path };
        store.migrate()?;
        Ok(store)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> AppResult<Self> {
        let manager = SqliteConnectionManager::memory().with_init(configure_connection);
        let pool = Pool::builder().max_size(1).build(manager)?;
        let store = Self {
            pool,
            path: PathBuf::from(":memory:"),
        };
        store.migrate()?;
        Ok(store)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn schema_version(&self) -> AppResult<i64> {
        self.with_reader(|connection| {
            connection
                .query_row(
                    "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
                    [],
                    |row| row.get(0),
                )
                .map_err(AppError::from)
        })
    }

    pub fn integrity_check(&self) -> AppResult<bool> {
        self.with_reader(|connection| {
            let result: String =
                connection.query_row("PRAGMA quick_check(1)", [], |row| row.get(0))?;
            Ok(result == "ok")
        })
    }

    pub fn database_size_bytes(&self) -> AppResult<u64> {
        if self.path == Path::new(":memory:") {
            return Ok(0);
        }
        Ok(std::fs::metadata(&self.path)?.len())
    }

    pub(crate) fn with_reader<T>(
        &self,
        operation: impl FnOnce(&Connection) -> AppResult<T>,
    ) -> AppResult<T> {
        let connection = self.connection()?;
        operation(&connection)
    }

    pub(crate) fn with_writer<T>(
        &self,
        operation: impl FnOnce(&Transaction<'_>) -> AppResult<T>,
    ) -> AppResult<T> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let value = operation(&transaction)?;
        transaction.commit()?;
        Ok(value)
    }

    fn connection(&self) -> AppResult<PooledConnection<SqliteConnectionManager>> {
        self.pool.get().map_err(AppError::from)
    }

    fn migrate(&self) -> AppResult<()> {
        let mut connection = self.connection()?;
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                applied_at_ms INTEGER NOT NULL
            ) STRICT;",
        )?;
        let current: i64 = connection.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )?;

        for (version, sql) in MIGRATIONS.iter().copied().filter(|(v, _)| *v > current) {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Exclusive)?;
            transaction.execute_batch(sql)?;
            transaction.execute(
                "INSERT INTO schema_migrations(version, name, applied_at_ms)
                 VALUES (?1, ?2, CAST(unixepoch('subsec') * 1000 AS INTEGER))",
                (version, format!("migration_{version:04}")),
            )?;
            transaction.commit()?;
        }
        Ok(())
    }
}

fn configure_connection(connection: &mut Connection) -> rusqlite::Result<()> {
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.set_prepared_statement_cache_capacity(128);
    connection.create_scalar_function(
        "canonical_model_name",
        1,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |context| {
            let model = context.get::<Option<String>>(0)?;
            Ok(model.map(|model| canonical_model_name(&model)))
        },
    )?;
    connection.create_scalar_function(
        "model_strength_key",
        1,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |context| {
            let model = context.get::<String>(0)?;
            Ok(model_strength_sort_key(&model))
        },
    )?;
    connection.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA temp_store = MEMORY;
         PRAGMA cache_size = -20000;
         PRAGMA wal_autocheckpoint = 1000;",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_v6_merging_model_identity_without_losing_retained_usage_or_session_counts() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("v6.sqlite3");
        let mut connection = Connection::open(&path).unwrap();
        configure_connection(&mut connection).unwrap();
        connection.execute_batch("CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at_ms INTEGER NOT NULL) STRICT;").unwrap();
        for (version, sql) in MIGRATIONS.iter().filter(|(version, _)| *version <= 6) {
            connection.execute_batch(sql).unwrap();
            connection
                .execute(
                    "INSERT INTO schema_migrations VALUES (?1,'previous',0)",
                    [version],
                )
                .unwrap();
        }
        connection.execute_batch("INSERT INTO workspaces(id,normalized_path,display_name,created_at_ms,updated_at_ms) VALUES ('w','C:/work/shared','shared',0,0);
            INSERT INTO sessions(id,workspace_id,synthetic_title,latest_model_raw,primary_model_raw,parser_version,updated_at_ms,total_tokens) VALUES ('mixed','w','Session','gpt-6.1-sol-fast','gpt-5',11,0,130),('base','w','Session','gpt-6.1-sol','gpt-6.1-sol',11,0,20);").unwrap();
        for (session, model, tokens, index) in [
            ("mixed", "gpt-6.1-sol", 40, 0),
            ("mixed", "gpt-6.1-sol-fast", 30, 1),
            ("mixed", "gpt-5", 60, 2),
            ("base", "gpt-6.1-sol", 20, 0),
        ] {
            connection.execute("INSERT INTO session_model_segments(session_id,segment_index,started_at_ms,ended_at_ms,model_provider,model_raw,pricing_model_id,input_tokens,estimated_cost_microusd) VALUES (?1,?4,0,1,'openai',?2,?2,?3,?3)",rusqlite::params![session,model,tokens,index]).unwrap();
            connection.execute("INSERT INTO session_daily_usage(session_id,local_date,timezone_id,day_start_utc_ms,day_end_utc_ms,workspace_id,model_provider,model_raw,pricing_model_id,archived,total_tokens,input_tokens,fresh_input_tokens,active_ms,priced_cost_microusd,priced_event_count,cache_write_tokens,reported_cost_microusd) VALUES (?1,'2026-10-01','UTC',0,1,'w','openai',?2,?2,0,?3,?3,?3,?3,?3,1,1,?3)",rusqlite::params![session,model,tokens]).unwrap();
        }
        connection.execute_batch("INSERT INTO daily_usage_rollups(agent,local_date,timezone_id,day_start_utc_ms,day_end_utc_ms,workspace_id,model_provider,model_raw,pricing_model_id,archived,session_count,total_tokens,active_ms,priced_cost_microusd) SELECT 'codex',local_date,timezone_id,0,1,'w','openai',model_raw,pricing_model_id,0,COUNT(DISTINCT session_id),SUM(total_tokens),SUM(active_ms),SUM(priced_cost_microusd) FROM session_daily_usage GROUP BY model_raw,pricing_model_id;
            INSERT INTO app_settings VALUES ('activity','{\"idleGapMinutes\":45}',0);").unwrap();
        for (provider, model, builtin, price) in [
            ("openai", "gpt-6.1-sol", 1, "2"),
            ("openai", "gpt-6.1-sol-fast", 0, "9"),
            ("gateway", "gpt-6.1-sol", 0, "3"),
            ("gateway", "gpt-6.1-sol-fast", 0, "4"),
        ] {
            connection.execute("INSERT INTO model_prices(pricing_id,provider,display_name,input_per_million_usd,output_per_million_usd,cache_read_per_million_usd,default_input_per_million_usd,default_output_per_million_usd,default_cache_read_per_million_usd,is_builtin,is_overridden,is_deleted,revision,created_at_ms,updated_at_ms) VALUES (?1,?2,?1,?3,'10','0.1',?3,'10','0.1',?4,0,0,1,0,0)",rusqlite::params![model,provider,price,builtin]).unwrap();
        }
        drop(connection);
        let store = UsageStore::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), 7);
        store.with_reader(|connection| {
            assert_eq!(connection.query_row("SELECT primary_model_raw FROM sessions WHERE id='mixed'",[],|r|r.get::<_,String>(0))?,"gpt-6.1-sol");
            let daily:(i64,i64,i64,i64,i64)=connection.query_row("SELECT total_tokens,active_ms,priced_cost_microusd,cache_write_tokens,reported_cost_microusd FROM session_daily_usage WHERE session_id='mixed' AND model_raw='gpt-6.1-sol'",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?;
            assert_eq!(daily,(70,70,70,2,70));
            assert_eq!(connection.query_row("SELECT input_per_million_usd FROM model_prices WHERE provider='openai' AND pricing_id='gpt-6.1-sol'",[],|r|r.get::<_,String>(0))?,"9");
            assert_eq!(connection.query_row("SELECT input_per_million_usd FROM model_prices WHERE provider='gateway' AND pricing_id='gpt-6.1-sol'",[],|r|r.get::<_,String>(0))?,"3");
            assert_eq!(connection.query_row("SELECT COUNT(*) FROM model_prices",[],|r|r.get::<_,i64>(0))?,2);
            let rollup:(i64,i64,i64)=connection.query_row("SELECT total_tokens,session_count,priced_cost_microusd FROM daily_usage_rollups WHERE model_raw='gpt-6.1-sol'",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
            assert_eq!(rollup,(90,2,90));
            assert_eq!(connection.query_row("SELECT SUM(total_tokens) FROM session_daily_usage",[],|r|r.get::<_,i64>(0))?,150);
            for table in ["session_model_segments","session_daily_usage","daily_usage_rollups"] {
                assert_eq!(connection.query_row(&format!("SELECT COUNT(*) FROM {table} WHERE model_raw LIKE '%-fast%' OR pricing_model_id LIKE '%-fast%'"),[],|r|r.get::<_,i64>(0))?,0);
            }
            Ok(())
        }).unwrap();
        assert!(store.integrity_check().unwrap());
        assert_eq!(
            store.app_setting("activity").unwrap().as_deref(),
            Some("{\"idleGapMinutes\":45}")
        );
    }

    #[test]
    fn creates_required_schema_and_passes_integrity_check() {
        let store = UsageStore::open_in_memory().expect("open test database");

        assert_eq!(store.schema_version().expect("schema version"), 7);
        assert!(store.integrity_check().expect("integrity check"));

        let required = [
            "source_files",
            "sync_runs",
            "workspaces",
            "sessions",
            "usage_events",
            "session_model_segments",
            "activity_segments",
            "tool_events",
            "daily_usage_rollups",
            "daily_tool_rollups",
            "model_prices",
            "app_settings",
        ];
        store
            .with_reader(|connection| {
                for table in required {
                    let count: i64 = connection.query_row(
                        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                        [table],
                        |row| row.get(0),
                    )?;
                    assert_eq!(count, 1, "missing table {table}");
                }
                Ok(())
            })
            .expect("inspect schema");
    }

    #[test]
    fn migrates_v5_preserving_each_agent_rollup_and_cache_summary() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("v5.sqlite3");
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch("CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at_ms INTEGER NOT NULL) STRICT;").unwrap();
        for (version, sql) in MIGRATIONS.iter().filter(|(version, _)| *version <= 5) {
            connection.execute_batch(sql).unwrap();
            connection
                .execute(
                    "INSERT INTO schema_migrations VALUES (?1, 'previous', 0)",
                    [version],
                )
                .unwrap();
        }
        connection.execute_batch("INSERT INTO workspaces(id,normalized_path,display_name,alias,created_at_ms,updated_at_ms) VALUES ('workspace','C:/work/shared','shared','My project',0,0)").unwrap();
        for (id, agent) in [
            ("existing-codex", "codex"),
            ("claude:existing", "claude"),
            ("opencode:existing", "opencode"),
        ] {
            connection.execute("INSERT INTO sessions(id,workspace_id,synthetic_title,parser_version,updated_at_ms,total_tokens,cache_write_tokens,reported_cost_microusd) VALUES (?1,'workspace','Session',11,0,123,17,456)",[id]).unwrap();
            connection.execute("INSERT INTO daily_usage_rollups(agent,local_date,timezone_id,day_start_utc_ms,day_end_utc_ms,workspace_id,model_provider,model_raw,archived,total_tokens,cache_write_tokens,reported_cost_microusd) VALUES (?1,'2026-10-26','UTC',0,1,'workspace','provider','model',0,123,17,456)",[agent]).unwrap();
            connection.execute("INSERT INTO daily_tool_rollups(agent,local_date,timezone_id,day_start_utc_ms,day_end_utc_ms,workspace_id,archived,tool_name,category,operation_kind,call_count,session_count) VALUES (?1,'2026-10-26','UTC',0,1,'workspace',0,'bash','execute','unknown',7,1)",[agent]).unwrap();
        }
        drop(connection);
        let store = UsageStore::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), 7);
        store.with_reader(|connection| {
            for agent in ["codex","claude","opencode"] {
                let session:(i64,i64,i64) = connection.query_row("SELECT total_tokens,cache_write_tokens,reported_cost_microusd FROM sessions WHERE agent=?1",[agent],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?)))?;
                assert_eq!(session,(123,17,456));
                let rollup:(i64,i64,i64) = connection.query_row("SELECT total_tokens,cache_write_tokens,reported_cost_microusd FROM daily_usage_rollups WHERE agent=?1",[agent],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?)))?;
                assert_eq!(rollup,session);
                assert_eq!(connection.query_row("SELECT call_count FROM daily_tool_rollups WHERE agent=?1",[agent],|row|row.get::<_,i64>(0))?,7);
            }
            Ok(())
        }).unwrap();
        assert!(store.integrity_check().unwrap());
    }

    #[test]
    fn migrates_v4_preserving_codex_ids_rollups_aliases_prices_and_settings() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("v4.sqlite3");
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch("CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at_ms INTEGER NOT NULL) STRICT;").unwrap();
        for (version, sql) in MIGRATIONS.iter().filter(|(version, _)| *version <= 4) {
            connection.execute_batch(sql).unwrap();
            connection
                .execute(
                    "INSERT INTO schema_migrations VALUES (?1, 'previous', 0)",
                    [version],
                )
                .unwrap();
        }
        connection.execute_batch("INSERT INTO workspaces(id,normalized_path,display_name,alias,created_at_ms,updated_at_ms) VALUES ('workspace','C:/work/shared','shared','My project',0,0);
            INSERT INTO sessions(id,workspace_id,synthetic_title,parser_version,updated_at_ms,total_tokens) VALUES ('existing-codex','workspace','Session existing',11,0,123);
            INSERT INTO daily_usage_rollups(local_date,timezone_id,day_start_utc_ms,day_end_utc_ms,workspace_id,model_provider,model_raw,archived,total_tokens,priced_cost_microusd) VALUES ('2026-07-10','UTC',0,1,'workspace','openai','gpt-5',0,123,456);
            INSERT INTO model_prices(pricing_id,provider,display_name,input_per_million_usd,output_per_million_usd,cache_read_per_million_usd,default_input_per_million_usd,default_output_per_million_usd,default_cache_read_per_million_usd,is_builtin,is_overridden,is_deleted,revision,created_at_ms,updated_at_ms) VALUES ('gpt-5','openai','GPT-5','99','100','1','1','2','0.1',1,1,0,1,0,0);
            INSERT INTO app_settings(key,value_json,updated_at_ms) VALUES ('activity','{\"idleGapMinutes\":45}',0);").unwrap();
        drop(connection);
        let store = UsageStore::open(&path).unwrap();
        store.seed_builtin_prices().unwrap();
        store
            .with_reader(|connection| {
                let session: (String, String) =
                    connection.query_row("SELECT id,agent FROM sessions", [], |row| {
                        Ok((row.get(0)?, row.get(1)?))
                    })?;
                assert_eq!(session, ("existing-codex".into(), "codex".into()));
                let rollup: (i64, i64, String) = connection.query_row(
                    "SELECT total_tokens,priced_cost_microusd,agent FROM daily_usage_rollups",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )?;
                assert_eq!(rollup, (123, 456, "codex".into()));
                assert_eq!(
                    connection.query_row("SELECT alias FROM workspaces", [], |row| row
                        .get::<_, String>(0))?,
                    "My project"
                );
                Ok(())
            })
            .unwrap();
        assert_eq!(
            store
                .model_prices(false)
                .unwrap()
                .iter()
                .find(|price| price.provider == "openai" && price.pricing_id == "gpt-5")
                .unwrap()
                .input_per_million_usd,
            "99"
        );
        assert_eq!(
            store.app_setting("activity").unwrap().as_deref(),
            Some("{\"idleGapMinutes\":45}")
        );
        assert!(store.integrity_check().unwrap());
    }

    #[test]
    fn opens_file_database_in_wal_mode() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let store = UsageStore::open(directory.path().join("usage.sqlite3")).expect("open store");

        store
            .with_reader(|connection| {
                let mode: String =
                    connection.query_row("PRAGMA journal_mode", [], |row| row.get(0))?;
                assert_eq!(mode, "wal");
                Ok(())
            })
            .expect("read journal mode");
    }

    #[test]
    fn migrates_v3_first_token_latency_without_losing_checkpoints() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("v3.sqlite3");
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch("CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at_ms INTEGER NOT NULL) STRICT;").unwrap();
        for (version, sql) in MIGRATIONS.iter().filter(|(version, _)| *version <= 3) {
            connection.execute_batch(sql).unwrap();
            connection
                .execute(
                    "INSERT INTO schema_migrations VALUES (?1, 'previous', 0)",
                    [version],
                )
                .unwrap();
        }
        connection.execute_batch("INSERT INTO source_files (source_key, relative_path, source_kind, file_size, mtime_ns, prefix_hash, parser_version, safe_offset, complete_line_offset, open_task_turn_id, open_task_started_at_ms) VALUES ('jsonl:existing', 'sessions/existing.jsonl', 'session', 1000, 1, 'hash', 10, 123, 123, 'existing-turn', 1000);").unwrap();
        drop(connection);
        let store = UsageStore::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), 7);
        let checkpoint = store.source_checkpoints().unwrap().remove(0);
        assert_eq!(checkpoint.cursor.safe_offset, 123);
        assert_eq!(checkpoint.parser_version, 10);
        assert_eq!(
            checkpoint.cursor.open_task_turn_id.as_deref(),
            Some("existing-turn")
        );
        assert!(checkpoint.cursor.first_token_context.model_raw.is_none());
        assert!(store.integrity_check().unwrap());
    }

    #[test]
    fn migrates_v2_user_timing_without_losing_existing_source_checkpoint() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("v2.sqlite3");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(include_str!("../../migrations/0001_initial.sql"))
            .unwrap();
        connection
            .execute_batch(include_str!(
                "../../migrations/0002_embedded_transcript.sql"
            ))
            .unwrap();
        connection.execute_batch("CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at_ms INTEGER NOT NULL) STRICT; INSERT INTO schema_migrations VALUES (2, 'migration_0002', 0); INSERT INTO source_files (source_key, relative_path, source_kind, file_size, mtime_ns, prefix_hash, parser_version, safe_offset, complete_line_offset, open_task_turn_id, open_task_started_at_ms, contains_embedded_history) VALUES ('jsonl:existing', 'sessions/existing.jsonl', 'session', 1000, 1, 'hash', 8, 123, 123, 'existing-turn', 1000, 1);").unwrap();
        drop(connection);
        let store = UsageStore::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), 7);
        let checkpoint = store.source_checkpoints().unwrap().remove(0);
        assert_eq!(checkpoint.cursor.safe_offset, 123);
        assert_eq!(
            checkpoint.cursor.open_task_turn_id.as_deref(),
            Some("existing-turn")
        );
        assert_eq!(checkpoint.cursor.open_task_started_at_ms, Some(1000));
        assert!(checkpoint.cursor.contains_embedded_history);
        assert!(
            checkpoint
                .cursor
                .user_wait_timing
                .excluded_intervals()
                .is_empty()
        );
        assert!(store.integrity_check().unwrap());
    }

    #[test]
    fn migrates_v1_database_with_embedded_transcript_checkpoint() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("usage.sqlite3");
        let connection = Connection::open(&path).expect("open v1 database");
        connection
            .execute_batch(include_str!("../../migrations/0001_initial.sql"))
            .expect("create v1 schema");
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                    version INTEGER PRIMARY KEY,
                    name TEXT NOT NULL,
                    applied_at_ms INTEGER NOT NULL
                 ) STRICT;
                 INSERT INTO schema_migrations(version, name, applied_at_ms)
                 VALUES (1, 'migration_0001', 0);",
            )
            .expect("mark v1 migration");
        drop(connection);

        let store = UsageStore::open(&path).expect("migrate store");
        assert_eq!(store.schema_version().expect("schema version"), 7);
        store
            .with_reader(|connection| {
                let column_exists: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM pragma_table_info('source_files')
                     WHERE name = 'contains_embedded_history'",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(column_exists, 1);
                let timing_column: i64 = connection.query_row("SELECT COUNT(*) FROM pragma_table_info('source_files') WHERE name = 'user_wait_timing_json'", [], |row| row.get(0))?;
                assert_eq!(timing_column, 1);
                Ok(())
            })
            .expect("inspect migrated source schema");
    }
}
