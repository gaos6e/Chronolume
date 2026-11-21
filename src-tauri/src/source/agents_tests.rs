use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use chrono::Utc;
use rusqlite::{Connection, params};
use serde_json::{Value, json};

use super::agents::{MultiAgentSource, SourceSettings};
use super::{ParsedRecord, UsageSource};
use crate::export::{ExportFormat, ExportPrivacy, ExportRequest, ExportScope, UsageExporter};
use crate::indexer::{SyncMode, UsageIndexer};
use crate::pricing::{PriceQuote, PriceableTokens, PricingCatalog, builtin_model_prices};
use crate::query::{
    ArchiveFilter, HeatmapMetric, HeatmapQuery, HeatmapSpan, ListQuery, RangePreset,
    RangeSelection, UsageFilters, UsageQuery,
};
use crate::store::{ModelPriceInput, UsageStore};

const SECRET: &str = "PRIVATE_AGENT_MARKER_DO_NOT_PERSIST";

#[test]
fn repair_replays_only_bad_sources_and_keeps_deleted_source_history() {
    let directory = tempfile::tempdir().unwrap();
    let now = Utc::now().timestamp_millis();
    for name in ["healthy", "removed", "error", "cursor", "summary"] {
        let time = if name == "removed" {
            now - 120 * 86_400_000
        } else {
            now
        };
        write_lines(
            &directory
                .path()
                .join(format!("claude/projects/p/{name}.jsonl")),
            &[claude_message(name, time, 100, 10)],
        );
    }
    let (store, _, indexer) = setup(directory.path());
    assert_eq!(indexer.sync(SyncMode::Initial).unwrap().file_errors, 0);
    fs::remove_file(directory.path().join("claude/projects/p/removed.jsonl")).unwrap();
    let healthy_updated: i64 = store
        .with_reader(|connection| {
            Ok(connection.query_row(
                "SELECT updated_at_ms FROM sessions WHERE id='claude:healthy'",
                [],
                |row| row.get(0),
            )?)
        })
        .unwrap();
    store.with_writer(|transaction| {
        transaction.execute("UPDATE source_files SET status='error' WHERE session_id='claude:error'", [])?;
        transaction.execute("UPDATE source_files SET user_wait_timing_json='invalid-json' WHERE session_id='claude:cursor'", [])?;
        transaction.execute("UPDATE sessions SET total_tokens=1 WHERE id='claude:summary'", [])?;
        Ok(())
    }).unwrap();

    let repaired = indexer.sync(SyncMode::Repair).unwrap();
    assert_eq!(repaired.file_errors, 0);
    // Three repaired sources and one missing source; the healthy file is untouched.
    assert_eq!(repaired.files_total, 4);
    assert!(store.source_checkpoints().is_ok());
    store.with_reader(|connection| {
        let totals: (i64, i64) = connection.query_row("SELECT COUNT(*),SUM(total_tokens) FROM sessions", [], |row| Ok((row.get(0)?, row.get(1)?)))?;
        assert_eq!(totals, (5, 700));
        let after: i64 = connection.query_row("SELECT updated_at_ms FROM sessions WHERE id='claude:healthy'", [], |row| row.get(0))?;
        assert_eq!(after, healthy_updated);
        let retained: i64 = connection.query_row("SELECT COUNT(*) FROM usage_events WHERE session_id='claude:removed'", [], |row| row.get(0))?;
        assert_eq!(retained, 0);
        let history: i64 = connection.query_row("SELECT SUM(total_tokens) FROM session_daily_usage WHERE session_id='claude:removed'", [], |row| row.get(0))?;
        assert_eq!(history, 140);
        let global: i64 = connection.query_row("SELECT SUM(total_tokens) FROM daily_usage_rollups", [], |row| row.get(0))?;
        assert_eq!(global, 700);
        Ok(())
    }).unwrap();
    assert_eq!(indexer.sync(SyncMode::Repair).unwrap().records_written, 0);
}

#[test]
fn failed_repair_preserves_previous_snapshot_and_can_retry_unchanged_source() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("claude/projects/p/broken.jsonl");
    let message = claude_message("repair-message", Utc::now().timestamp_millis(), 100, 10);
    write_lines(&path, std::slice::from_ref(&message));
    let (store, _, indexer) = setup(directory.path());
    indexer.sync(SyncMode::Initial).unwrap();
    fs::write(&path, b"invalid-json\n").unwrap();
    let failed = indexer.sync(SyncMode::Repair).unwrap();
    assert_eq!(failed.file_errors, 1);
    assert!(
        failed
            .error_counts
            .keys()
            .any(|key| key.contains("repair_source_incomplete"))
    );
    store
        .with_reader(|connection| {
            let tokens: i64 = connection.query_row(
                "SELECT total_tokens FROM sessions WHERE id='claude:broken'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(tokens, 140);
            let events: i64 =
                connection.query_row("SELECT SUM(total_tokens) FROM usage_events", [], |row| {
                    row.get(0)
                })?;
            assert_eq!(events, 140);
            Ok(())
        })
        .unwrap();
    // Restore exactly the previously indexed content. The stored error flag
    // must still force a replay rather than treating it as an unchanged source.
    write_lines(&path, &[message]);
    let repaired = indexer.sync(SyncMode::Repair).unwrap();
    assert_eq!(repaired.file_errors, 0);
    assert!(repaired.records_written > 0);
}

#[test]
fn repair_never_resets_healthy_records_when_database_check_fails() {
    let directory = tempfile::tempdir().unwrap();
    write_lines(
        &directory.path().join("claude/projects/p/healthy.jsonl"),
        &[claude_message(
            "healthy",
            Utc::now().timestamp_millis(),
            100,
            10,
        )],
    );
    let (store, _, indexer) = setup(directory.path());
    indexer.sync(SyncMode::Initial).unwrap();
    store
        .with_writer(|transaction| {
            transaction.execute_batch(
                "PRAGMA ignore_check_constraints=ON;
            UPDATE source_files SET file_size=-1;
            PRAGMA ignore_check_constraints=OFF;",
            )?;
            Ok(())
        })
        .unwrap();
    assert!(!store.integrity_check().unwrap());
    assert_eq!(
        indexer.sync(SyncMode::Repair).unwrap_err().code,
        "repair_database_damaged"
    );
    assert_eq!(indexer.status().phase, crate::indexer::SyncPhase::Failed);
    store
        .with_reader(|connection| {
            let tokens: i64 = connection.query_row(
                "SELECT total_tokens FROM sessions WHERE id='claude:healthy'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(tokens, 140);
            Ok(())
        })
        .unwrap();
}

#[test]
fn reindex_backs_up_deleted_source_history_before_rebuilding() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("claude/projects/p/removed.jsonl");
    write_lines(
        &path,
        &[claude_message(
            "removed",
            Utc::now().timestamp_millis(),
            100,
            10,
        )],
    );
    let (store, _, indexer) = setup(directory.path());
    indexer.sync(SyncMode::Initial).unwrap();
    fs::remove_file(path).unwrap();
    assert_eq!(indexer.sync(SyncMode::Rebuild).unwrap().file_errors, 0);
    let backups: Vec<_> = fs::read_dir(store.path().parent().unwrap().join("backups"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(backups.len(), 1);
    assert!(
        backups[0]
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains("-reindex-")
    );
    let backup =
        Connection::open_with_flags(&backups[0], rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let tokens: i64 = backup
        .query_row(
            "SELECT total_tokens FROM sessions WHERE id='claude:removed'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(tokens, 140);
    store
        .with_reader(|connection| {
            let count: i64 =
                connection.query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))?;
            assert_eq!(count, 0);
            Ok(())
        })
        .unwrap();
}

fn write_json(path: &Path, value: Value) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
}

fn write_lines(path: &Path, values: &[Value]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut file = fs::File::create(path).unwrap();
    for value in values {
        writeln!(file, "{value}").unwrap();
    }
}

fn claude_message(id: &str, time: i64, input: u64, output: u64) -> Value {
    json!({"type":"assistant", "sessionId":"same-id", "cwd":"C:/work/shared",
        "requestId":format!("request-{id}"), "message":{"id":id,"model":"claude-sonnet-4-6","usage":{
            "input_tokens":input,"output_tokens":output,"cache_read_input_tokens":20,
            "cache_creation_input_tokens":10,"cache_creation":{"ephemeral_5m_input_tokens":6,"ephemeral_1h_input_tokens":4}},
            "content":[{"type":"text","text":SECRET},{"type":"tool_use","id":format!("tool-{id}"),"name":"Edit","input":{"path":SECRET,"code":SECRET}}]},
        "timestamp":chrono::DateTime::from_timestamp_millis(time).unwrap().to_rfc3339()})
}

fn opencode_db(path: &Path, time: i64, v2: bool) -> Connection {
    let connection = Connection::open(path).unwrap();
    connection.execute_batch("PRAGMA journal_mode=WAL;
        CREATE TABLE session (id TEXT PRIMARY KEY, directory TEXT, parent_id TEXT, time_created INTEGER, time_updated INTEGER, time_archived INTEGER, title TEXT);
        CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, time_updated INTEGER, data TEXT);
        CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT, session_id TEXT, time_created INTEGER, time_updated INTEGER, data TEXT);").unwrap();
    connection
        .execute(
            "INSERT INTO session VALUES ('same-id', 'C:/work/shared', NULL, ?1, ?1, NULL, ?2)",
            params![time, SECRET],
        )
        .unwrap();
    if v2 {
        connection.execute_batch("CREATE TABLE session_message (id TEXT PRIMARY KEY, session_id TEXT, type TEXT, time_created INTEGER, time_updated INTEGER, data TEXT)").unwrap();
    }
    connection
}

fn oc_message(time: i64, output: u64) -> Value {
    json!({"id":"oc-message","sessionID":"same-id","role":"assistant","modelID":"claude-sonnet-4-6","providerID":"anthropic",
        "tokens":{"input":30,"output":output,"reasoning":5,"cache":{"read":20,"write":10}},
        "time":{"created":time,"completed":time+1000},"cost":0.125,"text":SECRET})
}

fn filters(agent: Option<&str>) -> UsageFilters {
    UsageFilters {
        agent: agent.map(str::to_owned),
        range: RangeSelection {
            preset: RangePreset::All,
            start_ms: None,
            end_ms: None,
            live_end: false,
        },
        workspace_id: None,
        model_provider: None,
        model_vendors: Vec::new(),
        model: None,
        archived: ArchiveFilter::All,
    }
}

fn list(agent: Option<&str>) -> ListQuery {
    ListQuery {
        filters: filters(agent),
        workspace_ids: None,
        search: String::new(),
        sort: "recent".into(),
        descending: true,
        page: 0,
        page_size: 30,
    }
}

fn setup(root: &Path) -> (Arc<UsageStore>, Arc<MultiAgentSource>, UsageIndexer) {
    let store = Arc::new(UsageStore::open(root.join("analysis.sqlite3")).unwrap());
    store.seed_builtin_prices().unwrap();
    let source = Arc::new(MultiAgentSource::new(SourceSettings {
        codex: Some(root.join("codex").to_string_lossy().into()),
        claude: Some(root.join("claude").to_string_lossy().into()),
        opencode: Some(root.join("opencode").to_string_lossy().into()),
        dsh: Some(root.join("dsh").to_string_lossy().into()),
        zcode: Some(root.join("zcode").to_string_lossy().into()),
        antigravity: Some(root.join("antigravity").to_string_lossy().into()),
        antigravity_cli: Some(root.join("antigravity-cli").to_string_lossy().into()),
        antigravity_ide: Some(root.join("antigravity-ide").to_string_lossy().into()),
        workbuddy: Some(root.join("workbuddy").to_string_lossy().into()),
        workbuddy_ai: Some(root.join("workbuddy-ai").to_string_lossy().into()),
        workbuddy_cli: Some(root.join("codebuddy-cli").to_string_lossy().into()),
    }));
    let indexer = UsageIndexer::new(
        store.clone(),
        source.clone(),
        store.pricing_catalog().unwrap(),
        chrono_tz::UTC,
        30 * 60 * 1000,
    );
    (store, source, indexer)
}

#[test]
fn antigravity_imports_three_clients_and_replays_corrected_steps_once() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let logs = |client: &str, id: &str| {
        root.join(client)
            .join("brain")
            .join(id)
            .join(".system_generated/logs")
    };
    let app = logs("antigravity", "parent").join("transcript.jsonl");
    let cli = logs("antigravity-cli", "parent").join("transcript_full.jsonl");
    let ide = logs("antigravity-ide", "child").join("transcript.jsonl");
    let time = Utc::now().to_rfc3339();
    let step = |index, input, cached, output| {
        json!({
            "step_index":index, "created_at":time, "type":"PLANNER_RESPONSE", "status":"DONE",
            "model":if index == 1 { "gemini-4-neon-high" } else { "gemini-4-neon-medium" },
            "input_tokens":input, "cache_read_tokens":cached, "output_tokens":output,
            "content":SECRET, "thinking":SECRET,
            "tool_calls":[{"name":"run_command","args":SECRET},{"name":"run_command","args":SECRET}]
        })
    };
    let first = step(1, 10, 50, 7);
    write_lines(&app, &[first.clone(), first.clone()]);
    write_lines(
        &app.with_file_name("transcript_full.jsonl"),
        &[step(9, 999, 0, 0)],
    );
    write_lines(&cli, &[first, step(2, 5, 0, 3)]);
    write_lines(&ide, &[step(1, 1, 2, 3)]);
    let db = Connection::open(root.join("antigravity-ide/conversation_summaries.db")).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL;").unwrap();
    let uri = reqwest::Url::from_directory_path(root.join("shared")).unwrap();
    db.execute_batch("CREATE TABLE conversation_summaries(conversation_id TEXT PRIMARY KEY, workspace_uris TEXT, parent_conversation_id TEXT, preview TEXT)").unwrap();
    db.execute(
        "INSERT INTO conversation_summaries VALUES ('child',?1,'parent',?2)",
        params![uri.as_str(), SECRET],
    )
    .unwrap();

    let (store, source, indexer) = setup(root);
    let description = source
        .settings()
        .describe()
        .unwrap()
        .into_iter()
        .find(|s| s.agent == "antigravity")
        .unwrap();
    assert_eq!(description.roots.len(), 3);
    assert!(description.roots.iter().all(|r| r.available));
    let sync = indexer.sync(SyncMode::Initial).unwrap();
    assert_eq!(
        (sync.files_total, sync.file_errors, sync.parse_failures),
        (2, 0, 0),
        "{:?}",
        sync.error_counts
    );
    let query = Arc::new(UsageQuery::new(store.clone(), chrono_tz::UTC));
    let filter = filters(Some("antigravity"));
    assert_eq!(query.sessions(&list(Some("antigravity"))).unwrap().total, 2);
    let parent = query.session_detail("antigravity:parent").unwrap().session;
    assert!(parent.title.contains("parent"));
    assert_eq!(
        (
            parent.fresh_input_tokens,
            parent.cached_input_tokens,
            parent.output_tokens,
            parent.total_tokens
        ),
        (15, 50, 10, 75)
    );
    assert_eq!(parent.tokens_per_second, None);
    assert_eq!(parent.average_first_token_latency_ms, None);
    let child = query.session_detail("antigravity:child").unwrap().session;
    assert_eq!(
        child.parent_session_id.as_deref(),
        Some("antigravity:parent")
    );
    assert_eq!(child.workspace_label, "Google Antigravity-shared");
    assert_eq!(child.total_tokens, 6);
    let dashboard = query.dashboard(&filter).unwrap();
    assert_eq!(dashboard.hero.real_total_tokens, 81);
    let models = query.models(&list(Some("antigravity"))).unwrap();
    assert_eq!(models.total, 1);
    assert_eq!(models.items[0].model, "gemini-4-neon");
    assert_eq!(models.items[0].unpriced_event_count, 0);
    assert_eq!(models.items[0].cache_hit_rate, Some(52.0 / 68.0));
    assert_eq!(
        dashboard.hero.cache_hit_rate,
        models.items[0].cache_hit_rate
    );
    assert!(!dashboard.hero.active_timing_available);
    assert_eq!(query.tools(&filter).unwrap().total_calls, 6);
    let exporter = UsageExporter::new(query.clone());
    for (format, ext) in [(ExportFormat::Csv, "csv"), (ExportFormat::Json, "json")] {
        let path = root.join(format!("sessions.{ext}"));
        let result = exporter
            .export_to_path(
                &ExportRequest {
                    scope: ExportScope::Sessions,
                    format,
                    filters: filter.clone(),
                    privacy: ExportPrivacy::Anonymous,
                },
                path.to_str().unwrap(),
            )
            .unwrap();
        assert_eq!(result.rows_written, 2);
        let output = fs::read_to_string(path).unwrap();
        assert!(!output.contains(SECRET));
        assert!(output.contains("antigravity"));
        if ext == "json" {
            let envelope: Value = serde_json::from_str(&output).unwrap();
            let rows = envelope["data"].as_array().unwrap();
            assert!(
                rows.iter()
                    .all(|row| row["activeMs"] == Value::Null
                        && row["activeTimingAvailable"] == false)
            );
        }
    }
    assert_eq!(indexer.sync(SyncMode::Incremental).unwrap().files_total, 0);
    // A metadata-only edit must update the project even when the transcript is unchanged.
    db.execute("UPDATE conversation_summaries SET workspace_uris=''", [])
        .unwrap();
    assert_eq!(indexer.sync(SyncMode::Incremental).unwrap().files_total, 1);
    assert_eq!(
        query
            .session_detail("antigravity:child")
            .unwrap()
            .session
            .workspace_label,
        "Google Antigravity-(unknown)"
    );
    drop(db);
    let summary = root.join("antigravity-ide/conversation_summaries.db");
    let before = fs::read(&summary).unwrap();
    assert_eq!(indexer.sync(SyncMode::Incremental).unwrap().files_total, 0);
    assert_eq!(fs::read(&summary).unwrap(), before);
    assert!(!summary.with_extension("db-wal").exists());
    assert!(!summary.with_extension("db-shm").exists());
    let smaller = step(2, 2, 0, 1);
    write_lines(&cli, &[step(2, 5, 0, 3), smaller.clone()]);
    let mut file = fs::OpenOptions::new().append(true).open(&cli).unwrap();
    write!(file, "{{\"step_index\":").unwrap();
    drop(file);
    let sync = indexer.sync(SyncMode::Incremental).unwrap();
    assert_eq!((sync.files_total, sync.file_errors), (1, 0));
    assert_eq!(
        query
            .session_detail("antigravity:parent")
            .unwrap()
            .session
            .total_tokens,
        70
    );
    write_lines(&cli, &[smaller, step(3, 2, 0, 2)]);
    indexer.sync(SyncMode::Incremental).unwrap();
    assert_eq!(
        query
            .session_detail("antigravity:parent")
            .unwrap()
            .session
            .total_tokens,
        74
    );
    // A corrupt complete record must preserve the last successful snapshot.
    fs::write(&cli, b"{bad json}\n").unwrap();
    assert_eq!(indexer.sync(SyncMode::Incremental).unwrap().file_errors, 1);
    assert_eq!(
        query
            .session_detail("antigravity:parent")
            .unwrap()
            .session
            .total_tokens,
        74
    );
}

fn zcode_db(root: &Path, time: i64) -> Connection {
    let path = root.join("zcode/cli/db/db.sqlite");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let db = Connection::open(path).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL;
        CREATE TABLE session(id TEXT PRIMARY KEY,directory TEXT,parent_id TEXT,time_created INTEGER,time_updated INTEGER,time_archived INTEGER,title TEXT);
        CREATE TABLE model_usage(id TEXT PRIMARY KEY,session_id TEXT,started_at INTEGER,model_id TEXT,status TEXT,input_tokens INTEGER,output_tokens INTEGER,provider_id TEXT,cache_read_input_tokens INTEGER DEFAULT 0,cache_creation_input_tokens INTEGER DEFAULT 0,reasoning_tokens INTEGER DEFAULT 0,completed_at INTEGER,duration_ms INTEGER,turn_id TEXT,raw_usage_json TEXT);
        CREATE TABLE tool_usage(id TEXT PRIMARY KEY,session_id TEXT,tool_call_id TEXT,tool_name TEXT,started_at INTEGER,status TEXT,completed_at INTEGER,duration_ms INTEGER,error_message TEXT);
        CREATE TABLE turn_usage(session_id TEXT,turn_id TEXT,started_at INTEGER,status TEXT,first_token_at INTEGER,time_to_first_token_ms INTEGER,input_tokens INTEGER);
        CREATE TABLE message(id TEXT,data TEXT);").unwrap();
    db.execute(
        "INSERT INTO session VALUES ('same-id','C:/work/shared',NULL,?1,?1,NULL,?2)",
        params![time, SECRET],
    )
    .unwrap();
    db.execute(
        "INSERT INTO session VALUES ('child','C:/work/shared','same-id',?1,?1,?1,?2)",
        params![time, SECRET],
    )
    .unwrap();
    for (id, status, start, end, input, output, cached, write, reasoning) in [
        ("r1", "completed", 0, 1000, 1000, 200, 600, 100, 50),
        ("r2", "error", 2000, 2500, 100, 30, 20, 0, 0),
        ("r3", "cancelled", 3000, 3300, 50, 10, 0, 0, 0),
        ("r4", "running", 4000, 5000, 500000, 100000, 0, 0, 0),
    ] {
        db.execute("INSERT INTO model_usage VALUES (?1,'same-id',?2,'glm-5.3-flash',?3,?4,?5,'zai',?6,?7,?8,?9,NULL,'turn',?10)",params![id,time+start,status,input,output,cached,write,reasoning,time+end,SECRET]).unwrap();
    }
    for (id, call, name, start, end, status) in [
        ("t1", "tool1", "read_file", 900, 1600, "completed"),
        ("t1-copy", "tool1", "read_file", 900, 1600, "completed"),
        ("t2", "tool2", "bash", 2500, 2700, "error"),
    ] {
        db.execute(
            "INSERT INTO tool_usage VALUES (?1,'same-id',?2,?3,?4,?5,?6,NULL,?7)",
            params![id, call, name, time + start, status, time + end, SECRET],
        )
        .unwrap();
    }
    db.execute(
        "INSERT INTO turn_usage VALUES ('same-id','turn',?1,'completed',?2,100,999999)",
        params![time, time + 100],
    )
    .unwrap();
    db.execute("INSERT INTO message VALUES ('private',?1)", [SECRET])
        .unwrap();
    db
}

#[test]
fn zcode_ledger_preserves_usage_timing_parentage_privacy_and_wal_corrections() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let time = Utc::now().timestamp_millis() - 100_000;
    let db = zcode_db(root, time);
    let (store, source, indexer) = setup(root);
    let description = source
        .settings()
        .describe()
        .unwrap()
        .into_iter()
        .find(|s| s.agent == "zcode")
        .unwrap();
    assert!(description.available);
    let before = fs::read(root.join("zcode/cli/db/db.sqlite-wal")).unwrap();
    let status = indexer.sync(SyncMode::Initial).unwrap();
    assert_eq!(
        (status.file_errors, status.parse_failures),
        (0, 0),
        "{:?}",
        status.error_counts
    );
    let query = Arc::new(UsageQuery::new(store.clone(), chrono_tz::UTC));
    let sessions = query.sessions(&list(Some("zcode"))).unwrap();
    assert_eq!(sessions.total, 2);
    let detail = query.session_detail("zcode:same-id").unwrap();
    let session = detail.session;
    assert_eq!(
        (
            session.input_tokens,
            session.output_tokens,
            session.total_tokens
        ),
        (1150, 240, 1390)
    );
    assert_eq!(
        (
            session.fresh_input_tokens,
            session.cached_input_tokens,
            session.cache_write_tokens,
            session.reasoning_tokens
        ),
        (430, 620, 100, 50)
    );
    assert_eq!(session.active_ms, 2600);
    assert_eq!(session.active_method, "lifecycle");
    assert!(!session.active_is_estimate);
    assert_eq!(session.average_first_token_latency_ms, Some(100.0));
    assert_eq!(session.first_token_latency_sample_count, 1);
    assert_eq!(session.integrity_status, "partial");
    assert!(session.estimated_cost_microusd.unwrap() > 0);
    let child = query.session_detail("zcode:child").unwrap().session;
    assert_eq!(child.parent_session_id.as_deref(), Some("zcode:same-id"));
    assert!(child.archived);
    assert_eq!(query.tools(&filters(Some("zcode"))).unwrap().total_calls, 2);
    let dashboard = query.dashboard(&filters(Some("zcode"))).unwrap();
    assert_eq!(dashboard.hero.real_total_tokens, 1390);
    assert!(dashboard.hero.active_timing_available);
    assert_eq!(
        query.models(&list(Some("zcode"))).unwrap().items[0].total_tokens,
        1390
    );
    let exporter = UsageExporter::new(query.clone());
    let output = root.join("zcode-export.json");
    exporter
        .export_to_path(
            &ExportRequest {
                scope: ExportScope::Sessions,
                format: ExportFormat::Json,
                filters: filters(Some("zcode")),
                privacy: ExportPrivacy::Anonymous,
            },
            output.to_str().unwrap(),
        )
        .unwrap();
    let exported = fs::read_to_string(output).unwrap();
    assert!(exported.contains("\"agent\": \"zcode\""));
    assert!(!exported.contains(SECRET));
    assert!(!exported.contains("C:/work/shared"));
    assert_eq!(
        fs::read(root.join("zcode/cli/db/db.sqlite-wal")).unwrap(),
        before
    );
    assert_eq!(indexer.sync(SyncMode::Incremental).unwrap().files_total, 0);

    // Same row count, timestamp and field width; the WAL has not checkpointed.
    db.execute("UPDATE model_usage SET output_tokens=100 WHERE id='r1'", [])
        .unwrap();
    let changes = source.plan(&store.source_checkpoints().unwrap()).unwrap();
    assert_eq!(changes.changes.len(), 1);
    // Cancelling a replay preserves the last committed analytical snapshot.
    assert!(
        store
            .apply_source_change(
                source.as_ref(),
                &changes.changes[0],
                &store.pricing_catalog().unwrap(),
                chrono_tz::UTC,
                30 * 60 * 1000,
                &AtomicBool::new(true)
            )
            .is_err()
    );
    assert_eq!(
        query
            .session_detail("zcode:same-id")
            .unwrap()
            .session
            .total_tokens,
        1390
    );
    let status = indexer.sync(SyncMode::Incremental).unwrap();
    assert_eq!((status.files_total, status.file_errors), (1, 0));
    assert_eq!(
        query
            .session_detail("zcode:same-id")
            .unwrap()
            .session
            .total_tokens,
        1290
    );
    assert_eq!(indexer.sync(SyncMode::Incremental).unwrap().files_total, 0);
    store
        .with_reader(|connection| {
            assert_eq!(
                connection.query_row(
                    "SELECT COUNT(*) FROM usage_events WHERE agent='zcode'",
                    [],
                    |r| r.get::<_, i64>(0)
                )?,
                3
            );
            Ok(())
        })
        .unwrap();
    for entry in fs::read_dir(root).unwrap().flatten().filter(|e| {
        e.file_name()
            .to_string_lossy()
            .starts_with("analysis.sqlite3")
    }) {
        assert!(
            !fs::read(entry.path())
                .unwrap()
                .windows(SECRET.len())
                .any(|w| w == SECRET.as_bytes())
        );
    }
}

#[test]
fn zcode_legacy_ledger_has_unknown_timing_and_missing_sources_keep_history() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fs::create_dir_all(root.join("zcode/cli/db")).unwrap();
    let path = root.join("zcode/cli/db/db.sqlite");
    let db = Connection::open(&path).unwrap();
    db.execute_batch("CREATE TABLE session(id TEXT,directory TEXT);
        CREATE TABLE model_usage(id TEXT,session_id TEXT,started_at INTEGER,model_id TEXT,status TEXT,input_tokens INTEGER,output_tokens INTEGER);
        INSERT INTO session VALUES ('legacy','C:/work/shared');").unwrap();
    let time = Utc::now().timestamp_millis() - 100_000;
    for (id, start) in [("a", time), ("b", time + 20_000)] {
        db.execute(
            "INSERT INTO model_usage VALUES (?1,'legacy',?2,'glm-5.3-flash','completed',100,20)",
            params![id, start],
        )
        .unwrap();
    }
    drop(db);
    let (store, source, indexer) = setup(root);
    // An explicitly selected database file follows the same discovery route.
    let mut settings = source.settings();
    settings.zcode = Some(path.to_string_lossy().into());
    settings.validate().unwrap();
    source.configure(settings);
    assert_eq!(indexer.sync(SyncMode::Initial).unwrap().file_errors, 0);
    let query = UsageQuery::new(store.clone(), chrono_tz::UTC);
    let session = query.session_detail("zcode:legacy").unwrap().session;
    assert_eq!((session.total_tokens, session.active_ms), (240, 0));
    assert_eq!(session.active_method, "unknown");
    assert_eq!(session.tokens_per_second, None);
    assert_eq!(session.average_first_token_latency_ms, None);
    let dashboard = query.dashboard(&filters(Some("zcode"))).unwrap();
    assert!(!dashboard.hero.active_timing_available);
    assert!(!query.workspaces(&list(Some("zcode"))).unwrap().items[0].active_timing_available);
    assert_eq!(
        query.models(&list(Some("zcode"))).unwrap().items[0].average_tokens_per_second,
        None
    );
    fs::remove_file(&path).unwrap();
    assert_eq!(indexer.sync(SyncMode::Incremental).unwrap().file_errors, 0);
    assert_eq!(
        query
            .session_detail("zcode:legacy")
            .unwrap()
            .session
            .total_tokens,
        240
    );
}

#[test]
fn zcode_unsupported_schema_does_not_block_other_sources() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fs::create_dir_all(root.join("zcode/cli/db")).unwrap();
    let db = Connection::open(root.join("zcode/cli/db/db.sqlite")).unwrap();
    db.execute_batch("CREATE TABLE session(id TEXT,directory TEXT)")
        .unwrap();
    write_lines(
        &root.join("claude/projects/project/same-id.jsonl"),
        &[claude_message("cc", Utc::now().timestamp_millis(), 30, 40)],
    );
    let (store, _, indexer) = setup(root);
    let status = indexer.sync(SyncMode::Initial).unwrap();
    assert_eq!(
        status.error_counts.get("zcode:unsupported_zcode_schema"),
        Some(&1)
    );
    assert_eq!(
        UsageQuery::new(store, chrono_tz::UTC)
            .sessions(&list(Some("claude")))
            .unwrap()
            .total,
        1
    );
}

#[test]
fn agents_keep_separate_workspaces_and_never_persist_content() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let time = Utc::now().timestamp_millis() - 60_000;
    let claude = root.join("claude/projects/project/same-id.jsonl");
    write_lines(
        &claude,
        &[
            claude_message("cc-message", time, 30, 10),
            claude_message("cc-message", time, 30, 40),
        ],
    );
    write_lines(
        &root.join("claude/projects/project/same-id/subagents/agent-worker.jsonl"),
        &[claude_message("cc-child", time + 1000, 1, 2)],
    );
    fs::create_dir_all(root.join("opencode")).unwrap();
    let db = opencode_db(&root.join("opencode/opencode.db"), time, false);
    let payload = oc_message(time, 40);
    db.execute(
        "INSERT INTO message VALUES ('oc-message','same-id',?1,?1,?2)",
        params![time, payload.to_string()],
    )
    .unwrap();
    db.execute("INSERT INTO part VALUES ('part','oc-message','same-id',?1,?1,?2)",params![time,json!({"type":"tool","tool":"bash","callID":"oc-tool","input":SECRET,"state":{"output":SECRET}}).to_string()]).unwrap();
    write_json(
        &root.join("opencode/storage/message/same-id/oc-message.json"),
        oc_message(time, 5),
    );
    write_lines(
        &root.join("codex/sessions/rollout-same-id.jsonl"),
        &[
            json!({"type":"session_meta","timestamp":Utc::now().to_rfc3339(),"payload":{"id":"same-id","cwd":"C:/work/shared","model_provider":"openai"}}),
            json!({"type":"turn_context","timestamp":Utc::now().to_rfc3339(),"payload":{"model":"gpt-5.6-sol","model_provider":"openai"}}),
            json!({"type":"event_msg","timestamp":Utc::now().to_rfc3339(),"payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":20,"output_tokens":10,"reasoning_output_tokens":0,"total_tokens":110}}}}),
        ],
    );
    let (store, source, indexer) = setup(root);
    let status = indexer.sync(SyncMode::Initial).unwrap();
    assert_eq!(status.file_errors, 0, "{:?}", status.error_counts);
    let query = Arc::new(UsageQuery::new(store.clone(), chrono_tz::UTC));
    let workspaces = query.workspaces(&list(None)).unwrap();
    assert_eq!(workspaces.total, 3);
    for (label, sessions) in [
        ("Codex-shared", 1),
        ("Claude Code-shared", 2),
        ("OpenCode-shared", 1),
    ] {
        let workspace = workspaces.items.iter().find(|w| w.label == label).unwrap();
        let mut request = list(None);
        request.filters.workspace_id = Some(workspace.id.clone());
        assert_eq!(query.sessions(&request).unwrap().total, sessions);
        assert_eq!(
            query
                .dashboard(&request.filters)
                .unwrap()
                .hero
                .real_total_tokens,
            workspace.total_tokens
        );
    }
    assert_eq!(query.sessions(&list(None)).unwrap().total, 4);
    let cc = query.sessions(&list(Some("claude"))).unwrap();
    assert_eq!(cc.total, 2);
    let parent = cc
        .items
        .iter()
        .find(|s| s.parent_session_id.is_none())
        .unwrap();
    assert_eq!(parent.id, "claude:same-id");
    assert_eq!(parent.total_tokens, 100);
    assert_eq!(parent.fresh_input_tokens, 30);
    assert_eq!(parent.cache_write_tokens, 10);
    assert_eq!(parent.cache_write_1h_tokens, 4);
    assert_eq!(parent.average_first_token_latency_ms, None);
    assert_eq!(
        cc.items
            .iter()
            .find(|s| s.parent_session_id.is_some())
            .unwrap()
            .parent_session_id
            .as_deref(),
        Some("claude:same-id")
    );
    assert_eq!(
        query.tools(&filters(Some("claude"))).unwrap().total_calls,
        2
    );
    let oc = query
        .sessions(&list(Some("opencode")))
        .unwrap()
        .items
        .remove(0);
    assert_eq!(oc.total_tokens, 105);
    assert_eq!(oc.reported_cost_microusd, Some(125_000));
    assert_eq!(oc.active_ms, 1000);
    assert_eq!(
        query.tools(&filters(Some("opencode"))).unwrap().total_calls,
        1
    );
    assert_eq!(query.models(&list(Some("claude"))).unwrap().total, 1);
    let dashboard = query.dashboard(&filters(Some("opencode"))).unwrap();
    assert_eq!(dashboard.hero.real_total_tokens, 105);
    assert_eq!(dashboard.hero.cache_write_tokens, 10);
    assert_eq!(dashboard.filter_options.providers[0].value, "anthropic");
    assert_eq!(dashboard.filter_options.workspaces.len(), 1);
    assert_eq!(
        dashboard.filter_options.workspaces[0].label,
        "OpenCode-shared"
    );
    assert_eq!(
        query
            .heatmap(&HeatmapQuery {
                filters: filters(Some("opencode")),
                metric: HeatmapMetric::Tokens,
                span: HeatmapSpan::Week
            })
            .unwrap()
            .points
            .iter()
            .map(|p| p.total_tokens)
            .sum::<i64>(),
        105
    );
    assert_eq!(
        query
            .dashboard(&filters(Some("codex")))
            .unwrap()
            .hero
            .real_total_tokens,
        110
    );
    assert_eq!(indexer.sync(SyncMode::Incremental).unwrap().files_total, 0);
    assert!(
        source
            .detect()
            .unwrap()
            .statuses
            .iter()
            .any(|s| s.agent == "claude" && s.exists)
    );
    let mut file = fs::OpenOptions::new().append(true).open(&claude).unwrap();
    writeln!(file, "{}", claude_message("cc-message", time, 30, 50)).unwrap();
    drop(file);
    let status = indexer.sync(SyncMode::Incremental).unwrap();
    assert_eq!(status.file_errors, 0, "{:?}", status.error_counts);
    assert_eq!(
        query
            .session_detail("claude:same-id")
            .unwrap()
            .session
            .total_tokens,
        110
    );
    let exporter = UsageExporter::new(query.clone());
    let request = ExportRequest {
        scope: ExportScope::Sessions,
        format: ExportFormat::Json,
        filters: filters(Some("claude")),
        privacy: ExportPrivacy::Anonymous,
    };
    let export_path = root.join("usage.json");
    exporter
        .export_to_path(&request, export_path.to_str().unwrap())
        .unwrap();
    let export = fs::read_to_string(export_path).unwrap();
    assert!(export.contains("\"agent\": \"claude\""));
    assert!(!export.contains(SECRET));
    store
        .with_reader(|connection| {
            for table in [
                "sessions",
                "usage_events",
                "source_files",
                "tool_events",
                "session_model_segments",
                "session_daily_usage",
            ] {
                let mut statement = connection.prepare(&format!("SELECT * FROM {table}"))?;
                let columns = statement.column_count();
                let mut rows = statement.query([])?;
                while let Some(row) = rows.next()? {
                    for i in 0..columns {
                        if let Ok(text) = row.get::<_, String>(i) {
                            assert!(!text.contains(SECRET), "{table}");
                        }
                    }
                }
            }
            Ok(())
        })
        .unwrap();
    for suffix in ["analysis.sqlite3", "analysis.sqlite3-wal"] {
        if let Ok(bytes) = fs::read(root.join(suffix)) {
            assert!(!bytes.windows(SECRET.len()).any(|b| b == SECRET.as_bytes()));
        }
    }
}

#[test]
fn opencode_updates_in_wal_and_v2_messages_replace_legacy_and_survive_retention_reprice() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let time = Utc::now().timestamp_millis() - 100 * 86_400_000;
    fs::create_dir_all(root.join("opencode")).unwrap();
    let db = opencode_db(&root.join("opencode/opencode.db"), time, true);
    db.execute(
        "INSERT INTO message VALUES ('oc-message','same-id',?1,?1,?2)",
        params![time, oc_message(time, 1).to_string()],
    )
    .unwrap();
    let mut payload = oc_message(time, 30);
    payload["model"] = json!({"id":"claude-sonnet-4-6","providerID":"anthropic"});
    payload.as_object_mut().unwrap().remove("modelID");
    payload.as_object_mut().unwrap().remove("providerID");
    payload["content"] = json!([{"type":"text","text":SECRET},{"type":"tool","id":"tool-v2","name":"read","state":{"input":SECRET,"content":[SECRET]}}]);
    db.execute(
        "INSERT INTO session_message VALUES ('oc-message','same-id','assistant',?1,?1,?2)",
        params![time, payload.to_string()],
    )
    .unwrap();
    let (store, _, indexer) = setup(root);
    let status = indexer.sync(SyncMode::Initial).unwrap();
    assert_eq!(status.file_errors, 0, "{:?}", status.error_counts);
    let query = UsageQuery::new(store.clone(), chrono_tz::UTC);
    assert_eq!(
        query
            .session_detail("opencode:same-id")
            .unwrap()
            .session
            .total_tokens,
        95
    );
    assert_eq!(
        query.tools(&filters(Some("opencode"))).unwrap().total_calls,
        1
    );
    payload["tokens"]["output"] = json!(35);
    db.execute(
        "UPDATE session_message SET data=?1,time_updated=?2 WHERE id='oc-message'",
        params![payload.to_string(), time + 1],
    )
    .unwrap();
    let status = indexer.sync(SyncMode::Incremental).unwrap();
    assert_eq!(status.files_total, 1);
    assert_eq!(status.file_errors, 0, "{:?}", status.error_counts);
    assert_eq!(
        query
            .session_detail("opencode:same-id")
            .unwrap()
            .session
            .total_tokens,
        100
    );
    store
        .rebuild_rollups_and_prune(time + 100 * 86_400_000, 90, chrono_tz::UTC)
        .unwrap();
    assert_eq!(
        query
            .session_detail("opencode:same-id")
            .unwrap()
            .retained_event_count,
        0
    );
    store
        .save_model_price(&ModelPriceInput {
            provider: "anthropic".into(),
            pricing_id: "claude-sonnet-4-6".into(),
            display_name: "Sonnet".into(),
            input_per_million_usd: "4".into(),
            output_per_million_usd: "20".into(),
            cache_read_per_million_usd: "0.4".into(),
            cache_write_per_million_usd: Some("5".into()),
        })
        .unwrap();
    store.reprice_all(chrono_tz::UTC).unwrap();
    let row = query.session_detail("opencode:same-id").unwrap().session;
    assert_eq!(row.estimated_cost_microusd, Some(978));
    assert_eq!(row.reported_cost_microusd, Some(125_000));
    assert_eq!(
        query
            .dashboard(&filters(Some("opencode")))
            .unwrap()
            .hero
            .cache_write_tokens,
        10
    );
    assert_eq!(
        store
            .model_prices(false)
            .unwrap()
            .iter()
            .find(|p| p.provider == "anthropic" && p.pricing_id == "claude-sonnet-4-6")
            .unwrap()
            .input_per_million_usd,
        "4"
    );
}

#[test]
fn opencode_reads_session_v2_without_legacy_tables_and_merges_mixed_metadata() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let time = Utc::now().timestamp_millis() - 60_000;
    fs::create_dir_all(root.join("opencode")).unwrap();
    let db = opencode_db(&root.join("opencode/opencode.db"), time, true);
    db.execute_batch(
        "ALTER TABLE session RENAME TO session_v2; DROP TABLE message; DROP TABLE part;",
    )
    .unwrap();
    let mut payload = oc_message(time, 40);
    payload["model"] = json!({"id":"claude-sonnet-4-6","providerID":"anthropic"});
    payload.as_object_mut().unwrap().remove("modelID");
    payload.as_object_mut().unwrap().remove("providerID");
    payload["time"] = json!({"created":chrono::DateTime::from_timestamp_millis(time).unwrap().to_rfc3339(),"completed":chrono::DateTime::from_timestamp_millis(time+1000).unwrap().to_rfc3339()});
    payload["content"] = json!([{"type":"tool","id":"v2-tool","name":"read","state":{"input":SECRET,"content":[SECRET]}}]);
    db.execute(
        "INSERT INTO session_message VALUES ('oc-message','same-id','assistant',?1,?1,?2)",
        params![time, payload.to_string()],
    )
    .unwrap();
    let (store, _, indexer) = setup(root);
    let status = indexer.sync(SyncMode::Initial).unwrap();
    assert_eq!(status.file_errors, 0, "{:?}", status.error_counts);
    let query = UsageQuery::new(store.clone(), chrono_tz::UTC);
    let row = query.session_detail("opencode:same-id").unwrap().session;
    assert_eq!(row.total_tokens, 105);
    assert_eq!(row.active_ms, 1000);
    assert_eq!(row.model_provider, "anthropic");
    assert_eq!(
        query.tools(&filters(Some("opencode"))).unwrap().total_calls,
        1
    );
    assert_eq!(indexer.sync(SyncMode::Incremental).unwrap().files_total, 0);
    db.execute_batch("CREATE TABLE session (id TEXT PRIMARY KEY,directory TEXT,parent_id TEXT,time_created INTEGER,time_updated INTEGER,time_archived INTEGER); INSERT INTO session SELECT id,directory,parent_id,time_created,time_updated,time_archived FROM session_v2;").unwrap();
    let status = indexer.sync(SyncMode::Incremental).unwrap();
    assert_eq!(status.files_total, 1);
    assert_eq!(status.file_errors, 0, "{:?}", status.error_counts);
    assert_eq!(query.sessions(&list(Some("opencode"))).unwrap().total, 1);
    assert_eq!(
        query
            .dashboard(&filters(Some("opencode")))
            .unwrap()
            .hero
            .real_total_tokens,
        105
    );
}

#[test]
fn cancellation_rolls_back_replayed_agent_session_and_incomplete_jsonl_is_retried() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let path = root.join("claude/projects/p/session.jsonl");
    let time = Utc::now().timestamp_millis() - 60_000;
    write_lines(&path, &[claude_message("message", time, 10, 10)]);
    let (store, source, indexer) = setup(root);
    indexer.sync(SyncMode::Initial).unwrap();
    let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
    write!(file, "{{\"type\":\"assistant\"").unwrap();
    drop(file);
    let plan = source.plan(&store.source_checkpoints().unwrap()).unwrap();
    let cancel = AtomicBool::new(false);
    let result = source.stream(
        &plan.changes[0],
        &mut |_: ParsedRecord| {
            cancel.store(true, Ordering::Relaxed);
            Ok(())
        },
        &cancel,
    );
    assert_eq!(result.unwrap_err().code, "sync_cancelled");
    let cancelled = AtomicBool::new(true);
    assert!(
        store
            .apply_source_change(
                source.as_ref(),
                &plan.changes[0],
                &store.pricing_catalog().unwrap(),
                chrono_tz::UTC,
                30 * 60 * 1000,
                &cancelled
            )
            .is_err()
    );
    assert_eq!(
        UsageQuery::new(store.clone(), chrono_tz::UTC)
            .sessions(&list(Some("claude")))
            .unwrap()
            .total,
        1
    );
    let status = indexer.sync(SyncMode::Incremental).unwrap();
    assert_eq!(status.file_errors, 0);
    assert_eq!(
        UsageQuery::new(store.clone(), chrono_tz::UTC)
            .sessions(&list(Some("claude")))
            .unwrap()
            .items[0]
            .integrity_status,
        "partial"
    );
}

#[test]
fn an_unreadable_agent_reports_an_error_without_blocking_other_sources() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    write_lines(
        &root.join("claude/projects/p/session.jsonl"),
        &[claude_message(
            "message",
            Utc::now().timestamp_millis() - 60000,
            10,
            10,
        )],
    );
    fs::create_dir_all(root.join("opencode")).unwrap();
    fs::write(root.join("opencode/opencode.db"), b"unreadable source").unwrap();
    let (store, _, indexer) = setup(root);
    let status = indexer.sync(SyncMode::Initial).unwrap();
    assert_eq!(status.file_errors, 1);
    assert!(
        status
            .error_counts
            .keys()
            .any(|code| code.starts_with("opencode:"))
    );
    assert_eq!(
        UsageQuery::new(store, chrono_tz::UTC)
            .sessions(&list(Some("claude")))
            .unwrap()
            .total,
        1
    );
}

#[test]
fn anthropic_cache_write_durations_and_provider_prices_are_distinct() {
    let catalog = PricingCatalog::new(builtin_model_prices());
    assert_eq!(
        catalog.quote(
            "anthropic",
            "claude-sonnet-4-6-20260217",
            PriceableTokens {
                fresh_input: 1_000_000,
                cached_input: 1_000_000,
                output: 1_000_000,
                cache_write: 2_000_000,
                cache_write_1h: 1_000_000
            }
        ),
        PriceQuote::Priced {
            pricing_id: "claude-sonnet-4-6".into(),
            revision: crate::pricing::BUILTIN_PRICING_REVISION,
            total_microusd: 28_050_000
        }
    );
    assert_eq!(
        catalog.quote(
            "openrouter",
            "claude-sonnet-4-6",
            PriceableTokens {
                fresh_input: 1,
                cached_input: 0,
                output: 1,
                cache_write: 0,
                cache_write_1h: 0
            }
        ),
        PriceQuote::Priced {
            pricing_id: "claude-sonnet-4-6".into(),
            revision: crate::pricing::BUILTIN_PRICING_REVISION,
            total_microusd: 18,
        }
    );
}

fn write_dsh(path: &Path, rows: &[Value]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    if path.extension().is_some_and(|s| s == "zstd") {
        // Real dsh files concatenate a header frame and independent append frames.
        let mut bytes = zstd::stream::encode_all(format!("{}\n", rows[0]).as_bytes(), 1).unwrap();
        let body = rows[1..]
            .iter()
            .map(|r| format!("{r}\n"))
            .collect::<String>();
        bytes.extend(zstd::stream::encode_all(body.as_bytes(), 1).unwrap());
        fs::write(path, bytes).unwrap();
    } else {
        write_lines(path, rows);
    }
}

#[test]
fn dsh_compressed_usage_replaces_stream_samples_and_excludes_fork_history() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let time = Utc::now().timestamp_millis() - 60_000;
    let mut rows = vec![
        json!({"type":"session","version":0,"id":"parent","createdAt":time,"cwd":"C:/work/shared"}),
        json!({"type":"turn/start","seq":0,"time":time,"data":{"turn":1}}),
        json!({"type":"step/start","seq":1,"time":time,"data":{"turn":1,"step":1}}),
        json!({"type":"request/header","seq":2,"time":time,"data":{"header":{"config":{"provider":"deepseek-official","model":"deepseek-v4-pro"},"system":SECRET}}}),
        json!({"type":"reasoning-chunks","seq0":3,"time0":time+100,"data":{"turn":1,"step":1,"dt":[1,2],"texts":[SECRET,SECRET,SECRET]}}),
        json!({"type":"assistant/chunk","seq":6,"time":time+900,"data":{"turn":1,"step":1,"chunk":{"type":"usage","usage":{"inputTokens":100,"outputTokens":20,"cacheReadTokens":10,"cacheWriteTokens":5,"reasoningTokens":5}}}}),
        json!({"type":"assistant/message","seq":7,"time":time+1000,"data":{"turn":1,"step":1,"message":{"content":SECRET},"usage":{"inputTokens":30,"outputTokens":10,"cacheReadTokens":10,"cacheWriteTokens":5,"reasoningTokens":5}}}),
        json!({"type":"tool/call","seq":8,"time":time+1000,"data":{"turn":1,"step":1,"name":"bash","callId":"tool","arguments":SECRET}}),
        json!({"type":"step/end","seq":9,"time":time+2000,"data":{"turn":1,"step":1}}),
    ];
    write_dsh(
        &root.join("dsh/sessions/project/parent/session.jsonl.zstd"),
        &rows,
    );
    rows[0] = json!({"type":"session","version":0,"id":"child","createdAt":time+3000,"cwd":"C:/work/shared","parentSession":"parent","seedLength":10});
    rows.extend([
        json!({"type":"turn/start","seq":10,"time":time+3000,"data":{"turn":2}}),
        json!({"type":"step/start","seq":11,"time":time+3000,"data":{"turn":2,"step":1}}),
        json!({"type":"assistant/message","seq":12,"time":time+4000,"data":{"turn":2,"step":1,"usage":{"inputTokens":2,"outputTokens":3}}}),
        json!({"type":"step/end","seq":13,"time":time+4000,"data":{"turn":2,"step":1}}),
    ]);
    write_dsh(
        &root.join("dsh/sessions/project/child/session.jsonl.zstd"),
        &rows,
    );
    let (store, _, indexer) = setup(root);
    let status = indexer.sync(SyncMode::Initial).unwrap();
    assert_eq!(status.file_errors, 0, "{:?}", status.error_counts);
    let query = Arc::new(UsageQuery::new(store.clone(), chrono_tz::UTC));
    let parent = query.session_detail("dsh:parent").unwrap().session;
    assert_eq!(
        (
            parent.total_tokens,
            parent.fresh_input_tokens,
            parent.cached_input_tokens,
            parent.cache_write_tokens,
            parent.output_tokens,
            parent.reasoning_tokens
        ),
        (55, 30, 10, 5, 10, 5)
    );
    assert_eq!(parent.average_first_token_latency_ms, Some(100.0));
    assert_eq!(parent.active_ms, 2000);
    let child = query.session_detail("dsh:child").unwrap().session;
    assert_eq!(child.total_tokens, 5);
    assert_eq!(child.parent_session_id.as_deref(), Some("dsh:parent"));
    assert_eq!(query.tools(&filters(Some("dsh"))).unwrap().total_calls, 1);
    assert_eq!(
        query
            .dashboard(&filters(Some("dsh")))
            .unwrap()
            .hero
            .real_total_tokens,
        60
    );
    assert_eq!(indexer.sync(SyncMode::Incremental).unwrap().files_total, 0);
    let path = root.join("export.json");
    UsageExporter::new(query)
        .export_to_path(
            &ExportRequest {
                scope: ExportScope::Sessions,
                format: ExportFormat::Json,
                filters: filters(Some("dsh")),
                privacy: ExportPrivacy::Anonymous,
            },
            path.to_str().unwrap(),
        )
        .unwrap();
    let export = fs::read_to_string(path).unwrap();
    assert!(export.contains("\"agent\": \"dsh\""));
    assert!(!export.contains(SECRET));
    for suffix in ["analysis.sqlite3", "analysis.sqlite3-wal"] {
        if let Ok(bytes) = fs::read(root.join(suffix)) {
            assert!(!bytes.windows(SECRET.len()).any(|b| b == SECRET.as_bytes()));
        }
    }
}

#[test]
fn dsh_generations_embedded_attempts_retries_and_tagged_seed_cuts() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let time = Utc::now().timestamp_millis() - 60000;
    let path = root.join("dsh/sessions/p/current/session.v4.jsonl");
    write_dsh(
        &root.join("dsh/sessions/p/current/session.jsonl.zstd"),
        &[
            json!({"type":"session","version":0,"id":"current","createdAt":time}),
            json!({"type":"assistant/message","seq":0,"time":time,"data":{"turn":1,"step":1,"usage":{"inputTokens":999,"outputTokens":999}}}),
        ],
    );
    write_dsh(
        &path,
        &[
            json!({"type":"session","version":4,"id":"current","createdAt":time,"isSeeded":true,"parentSession":"parent"}),
            json!({"type":"request/context","seq":0,"time":time,"data":{"provider":"deepseek-official","model":"deepseek-v4-pro"}}),
            json!({"type":"assistant/message","seq":1,"time":time,"data":{"turn":0,"step":1,"usage":{"inputTokens":999,"outputTokens":999}}}),
            json!({"type":"session/end-seed","seq":2,"time":time,"data":{"inherited":true}}),
            json!({"type":"turn/start","seq":3,"time":time,"data":{"turn":1}}),
            json!({"type":"step/start","seq":4,"time":time,"data":{"turn":1,"step":1}}),
            json!({"type":"assistant/attempt","seq":5,"time":time+500,"data":{"turn":1,"step":1,"stream":[{"type":"chunk","time":time+100,"chunk":{"type":"usage","usage":{"inputTokens":20,"outputTokens":9}}},{"type":"chunk","time":time+400,"chunk":{"type":"usage","usage":{"inputTokens":10,"outputTokens":5}}}]}}),
            json!({"type":"llm/retry-started","seq":6,"time":time+500,"data":{"turn":1,"step":1}}),
            json!({"type":"assistant/message","seq":7,"time":time+1000,"data":{"turn":1,"step":1,"stream":[{"type":"reasoning-chunks","time0":time+600,"dt":[1],"texts":[SECRET,SECRET]},{"type":"chunk","time":time+900,"chunk":{"type":"usage","usage":{"inputTokens":2,"outputTokens":3}}}]}}),
            json!({"type":"step/end","seq":8,"time":time+1000,"data":{"turn":1,"step":1}}),
            json!({"type":"session/end-seed","seq":9,"time":time+1000,"data":{}}),
        ],
    );
    let (store, _, indexer) = setup(root);
    let status = indexer.sync(SyncMode::Initial).unwrap();
    assert_eq!(status.file_errors, 0, "{:?}", status.error_counts);
    assert_eq!(status.files_total, 1);
    let query = UsageQuery::new(store.clone(), chrono_tz::UTC);
    let session = query.session_detail("dsh:current").unwrap().session;
    assert_eq!(session.total_tokens, 20);
    assert_eq!(session.active_ms, 1000);
    assert_eq!(session.average_first_token_latency_ms, Some(600.0));
    // An unsupported latest generation reports an error and preserves the last snapshot.
    write_dsh(
        &root.join("dsh/sessions/p/current/session.v5.jsonl"),
        &[json!({"type":"session","version":5,"id":"current","createdAt":time})],
    );
    assert_eq!(indexer.sync(SyncMode::Incremental).unwrap().file_errors, 1);
    assert_eq!(
        query
            .session_detail("dsh:current")
            .unwrap()
            .session
            .total_tokens,
        20
    );
}

#[test]
fn dsh_raw_historical_generations_and_partial_tail_are_read_only() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let time = Utc::now().timestamp_millis() - 60000;
    for version in 1..=3 {
        let path = root.join(format!(
            "dsh/sessions/p/session-{version}/session.v{version}.jsonl"
        ));
        write_dsh(
            &path,
            &[
                json!({"type":"session","version":version,"id":format!("historical-{version}"),"createdAt":time}),
                json!({"type":"assistant/message","seq":0,"time":time,"data":{"turn":1,"step":1,"usage":{"inputTokens":2,"outputTokens":3}}}),
            ],
        );
        if version == 1 {
            fs::OpenOptions::new()
                .append(true)
                .open(path)
                .unwrap()
                .write_all(b"{\"type\":")
                .unwrap();
        }
    }
    let (store, _, indexer) = setup(root);
    let status = indexer.sync(SyncMode::Initial).unwrap();
    assert_eq!(status.file_errors, 0, "{:?}", status.error_counts);
    let query = UsageQuery::new(store, chrono_tz::UTC);
    assert_eq!(query.sessions(&list(Some("dsh"))).unwrap().total, 3);
    assert_eq!(
        query
            .dashboard(&filters(Some("dsh")))
            .unwrap()
            .hero
            .real_total_tokens,
        15
    );
    assert_eq!(
        query
            .session_detail("dsh:historical-1")
            .unwrap()
            .session
            .integrity_status,
        "partial"
    );
}

#[test]
fn dsh_compression_corruption_rolls_back_and_torn_append_keeps_complete_records() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let time = Utc::now().timestamp_millis() - 60000;
    let path = root.join("dsh/sessions/p/tail/session.jsonl.zstd");
    let rows = [
        json!({"type":"session","version":0,"id":"tail","createdAt":time}),
        json!({"type":"assistant/message","seq":0,"time":time,"data":{"turn":1,"step":1,"usage":{"inputTokens":2,"outputTokens":3}}}),
    ];
    write_dsh(&path, &rows);
    let original = fs::read(&path).unwrap();
    let (store, _, indexer) = setup(root);
    assert_eq!(indexer.sync(SyncMode::Initial).unwrap().file_errors, 0);
    let query = UsageQuery::new(store, chrono_tz::UTC);
    // A torn append is allowed only after complete, already readable records.
    let append = format!(
        "{}\n",
        json!({"type":"assistant/message","seq":1,"time":time+100,"data":{"turn":2,"step":1,"usage":{"inputTokens":4,"outputTokens":6}}})
    );
    let frame = zstd::stream::encode_all(append.as_bytes(), 1).unwrap();
    let mut torn = original.clone();
    torn.extend(&frame[..frame.len() / 2]);
    fs::write(&path, &torn).unwrap();
    let status = indexer.sync(SyncMode::Incremental).unwrap();
    assert_eq!(status.file_errors, 0, "{:?}", status.error_counts);
    assert_eq!(
        query
            .session_detail("dsh:tail")
            .unwrap()
            .session
            .total_tokens,
        5
    );
    assert_eq!(
        query
            .session_detail("dsh:tail")
            .unwrap()
            .session
            .integrity_status,
        "partial"
    );
    assert_eq!(fs::read(&path).unwrap(), torn);
    let mut corrupt = original;
    corrupt.extend(b"corrupt-compressed-frame");
    fs::write(&path, &corrupt).unwrap();
    assert_eq!(indexer.sync(SyncMode::Incremental).unwrap().file_errors, 1);
    assert_eq!(
        query
            .session_detail("dsh:tail")
            .unwrap()
            .session
            .total_tokens,
        5
    );
}

#[test]
fn official_peak_and_context_brackets_survive_repricing_and_event_retention() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let time = chrono::DateTime::parse_from_rfc3339("2026-09-28T01:00:00Z")
        .unwrap()
        .timestamp_millis();
    let mut rows = vec![
        json!({"type":"session","version":4,"id":"pricing","createdAt":time,"cwd":"C:/work/pricing","isSeeded":false}),
    ];
    for (index, offset, model, provider, input) in [
        (0, 0, "deepseek-v4-pro", "deepseek-official", 1_000_000),
        (
            1,
            4 * 3600000,
            "deepseek-v4-pro",
            "deepseek-official",
            1_000_000,
        ),
        (2, 5 * 3600000, "gemini-2.5-pro", "google", 100_000),
        (3, 6 * 3600000, "gemini-2.5-pro", "google", 200_001),
    ] {
        rows.extend([
            json!({"type":"request/context","seq":index*2,"time":time+offset,"data":{"provider":provider,"model":model}}),
            json!({"type":"assistant/message","seq":index*2+1,"time":time+offset,"data":{"turn":index,"step":1,"usage":{"inputTokens":input,"outputTokens":0}}}),
        ]);
    }
    write_dsh(&root.join("dsh/sessions/p/pricing/session.v4.jsonl"), &rows);
    let (store, _, indexer) = setup(root);
    assert_eq!(indexer.sync(SyncMode::Initial).unwrap().file_errors, 0);
    let query = UsageQuery::new(store.clone(), chrono_tz::UTC);
    let expected = query
        .session_detail("dsh:pricing")
        .unwrap()
        .session
        .estimated_cost_microusd;
    assert_eq!(expected, Some(2_605_002));
    for _ in 0..2 {
        store.reprice_all(chrono_tz::UTC).unwrap();
        assert_eq!(
            query
                .session_detail("dsh:pricing")
                .unwrap()
                .session
                .estimated_cost_microusd,
            expected
        );
    }
    store
        .rebuild_rollups_and_prune(time + 200 * 86_400_000, 90, chrono_tz::UTC)
        .unwrap();
    store
        .with_reader(|connection| {
            assert_eq!(
                connection.query_row(
                    "SELECT COUNT(*) FROM usage_events WHERE agent='dsh'",
                    [],
                    |r| r.get::<_, i64>(0)
                )?,
                0
            );
            Ok(())
        })
        .unwrap();
    store.reprice_all(chrono_tz::UTC).unwrap();
    assert_eq!(
        query
            .session_detail("dsh:pricing")
            .unwrap()
            .session
            .estimated_cost_microusd,
        expected
    );
    assert_eq!(
        query
            .dashboard(&filters(Some("dsh")))
            .unwrap()
            .hero
            .estimated_cost_microusd,
        expected
    );
    store
        .save_model_price(&ModelPriceInput {
            provider: "deepseek-official".into(),
            pricing_id: "deepseek-v4-pro".into(),
            display_name: "My route".into(),
            input_per_million_usd: "3".into(),
            output_per_million_usd: "4".into(),
            cache_read_per_million_usd: "0.1".into(),
            cache_write_per_million_usd: None,
        })
        .unwrap();
    store.reprice_all(chrono_tz::UTC).unwrap();
    assert_eq!(
        query
            .session_detail("dsh:pricing")
            .unwrap()
            .session
            .estimated_cost_microusd,
        Some(6_625_002)
    );
    store
        .restore_builtin_price("deepseek-official", "deepseek-v4-pro")
        .unwrap();
    store.reprice_all(chrono_tz::UTC).unwrap();
    assert_eq!(
        query
            .session_detail("dsh:pricing")
            .unwrap()
            .session
            .estimated_cost_microusd,
        expected
    );
}

#[test]
fn model_alias_import_append_queries_and_exports_use_one_canonical_identity() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let time = Utc::now().timestamp_millis() - 60_000;
    let path = root.join("codex/sessions/rollout-model-identity.jsonl");
    let line = |model: &str, total: u64| {
        vec![
            json!({"type":"turn_context","timestamp":chrono::DateTime::from_timestamp_millis(time).unwrap().to_rfc3339(),"payload":{"model":model}}),
            json!({"type":"event_msg","timestamp":chrono::DateTime::from_timestamp_millis(time).unwrap().to_rfc3339(),"payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":total,"cached_input_tokens":0,"output_tokens":0,"reasoning_output_tokens":0,"total_tokens":total}}}}),
        ]
    };
    let mut values = vec![
        json!({"type":"session_meta","timestamp":chrono::DateTime::from_timestamp_millis(time).unwrap().to_rfc3339(),"payload":{"id":"identity","cwd":"C:/work/shared","model_provider":"openai"}}),
    ];
    values.extend(line("gpt-6.1-sol-fast", 10));
    values.extend(line("gpt-6.1-sol", 20));
    write_lines(&path, &values);
    let (store, _, indexer) = setup(root);
    assert_eq!(indexer.sync(SyncMode::Initial).unwrap().file_errors, 0);
    let query = Arc::new(UsageQuery::new(store.clone(), chrono_tz::UTC));
    let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
    for row in line("gpt-6.1-sol-fast", 30) {
        writeln!(file, "{row}").unwrap();
    }
    drop(file);
    assert_eq!(indexer.sync(SyncMode::Incremental).unwrap().file_errors, 0);
    let models = query.models(&list(None)).unwrap();
    assert_eq!(models.total, 1);
    assert_eq!(models.items[0].model, "gpt-6.1-sol");
    assert_eq!(models.items[0].total_tokens, 30);
    let checkpoint = store
        .source_checkpoints()
        .unwrap()
        .into_iter()
        .find(|c| c.session_id.as_deref() == Some("identity"))
        .unwrap();
    assert_eq!(
        checkpoint.cursor.current_model_raw.as_deref(),
        Some("gpt-6.1-sol")
    );
    assert_eq!(
        checkpoint.cursor.first_token_context.model_raw.as_deref(),
        Some("gpt-6.1-sol")
    );
    let mut selected = list(None);
    selected.filters.model = Some("gpt-6.1-sol-fast".into());
    assert_eq!(query.sessions(&selected).unwrap().total, 1);
    assert_eq!(
        query
            .dashboard(&selected.filters)
            .unwrap()
            .hero
            .real_total_tokens,
        30
    );
    for format in [ExportFormat::Json, ExportFormat::Csv] {
        let output = root.join("export.txt");
        UsageExporter::new(query.clone())
            .export_to_path(
                &ExportRequest {
                    scope: ExportScope::Models,
                    format,
                    filters: filters(None),
                    privacy: ExportPrivacy::Anonymous,
                },
                output.to_str().unwrap(),
            )
            .unwrap();
        let content = fs::read_to_string(output).unwrap();
        assert!(content.contains("gpt-6.1-sol"));
        assert!(!content.contains("gpt-6.1-sol-fast"));
    }
}

#[test]
fn workbuddy_variants_merge_native_usage_skip_abnormal_calls_and_keep_missing_timing() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let time = Utc::now().timestamp_millis() - 120_000;
    let first = root.join("workbuddy/projects/shared/native-session.jsonl");
    let copy = root.join("workbuddy-ai/projects/shared/native-session.jsonl");
    let sample = |id: &str, stamp: i64| {
        json!({
            "type":"function_call", "id":format!("row-{id}"), "timestamp":stamp,
            "cwd":"C:/work/shared", "sessionId":"native-session", "callId":format!("call-{id}"), "name":"Read",
            "arguments":SECRET, "content":SECRET,
            "providerData":{"messageId":id,"model":"deepseek-v4.1-flash","requestModelId":"deepseek-v4.1-flash",
                "usage":{"inputTokens":1000,"outputTokens":100,"inputTokensDetails":[{"cached_tokens":200}],"outputTokensDetails":[{"reasoning_tokens":30}]},
                "rawUsage":{"prompt_tokens":1000,"completion_tokens":100,"prompt_cache_hit_tokens":200,"prompt_cache_miss_tokens":750,"prompt_cache_write_tokens":50,"credit":999999}},
            "message":{"usage":{"input_tokens":1000,"output_tokens":100,"cache_read_input_tokens":200}}
        })
    };
    let a = sample("a", time);
    let mut repeated = a.clone();
    repeated["id"] = json!("another-carrier");
    let mut abnormal = sample("abnormal", time + 1000);
    abnormal["status"] = json!("incomplete");
    let mut failed_tool = json!({"type":"function_call_result","timestamp":time+1500,"callId":"call-a","status":"failed","output":SECRET});
    failed_tool["cwd"] = json!("C:/work/shared");
    write_lines(&first, &[a.clone(), repeated, abnormal, failed_tool]);
    let mut b = sample("b", time + 60_000);
    b["providerData"]["usage"]["inputTokensDetails"] = json!([{"cached_tokens":200}]);
    b["providerData"]["requestModelId"] = json!("unknown");
    write_lines(&copy, &[a, b]);
    let database = Connection::open(root.join("workbuddy-ai/workbuddy.db")).unwrap();
    database.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE sessions(id TEXT,cwd TEXT,created_at INTEGER,updated_at INTEGER,status TEXT,deleted_at INTEGER); CREATE TABLE session_usage(session_id TEXT, used INTEGER, size INTEGER, credit_json TEXT);").unwrap();
    database
        .execute(
            "INSERT INTO sessions VALUES ('native-session','C:/work/shared',?1,?1,'archived',NULL)",
            [time],
        )
        .unwrap();
    database
        .execute(
            "INSERT INTO session_usage VALUES ('native-session',999999999,999999999,?1)",
            [SECRET],
        )
        .unwrap();
    let (store, source, indexer) = setup(root);
    let result = indexer.sync(SyncMode::Initial).unwrap();
    assert_eq!(result.parse_failures, 0);
    let query = Arc::new(UsageQuery::new(store.clone(), chrono_tz::UTC));
    let filters = list(Some("workbuddy")).filters;
    let dashboard = query.dashboard(&filters).unwrap();
    assert_eq!(dashboard.hero.real_total_tokens, 2200);
    assert_eq!(dashboard.hero.cached_input_tokens, 400);
    assert_eq!(dashboard.hero.cache_write_tokens, 100);
    assert_eq!(dashboard.hero.reasoning_tokens, 60);
    assert!(!dashboard.hero.active_timing_available);
    assert_eq!(dashboard.hero.active_ms, 0);
    let sessions = query.sessions(&list(Some("workbuddy"))).unwrap();
    assert_eq!(sessions.total, 1);
    assert_eq!(sessions.items[0].id, "workbuddy:native-session");
    assert!(sessions.items[0].archived);
    assert_eq!(sessions.items[0].tokens_per_second, None);
    assert_eq!(sessions.items[0].average_first_token_latency_ms, None);
    assert!(!sessions.items[0].active_timing_available);
    assert!(!query.workspaces(&list(Some("workbuddy"))).unwrap().items[0].active_timing_available);
    store
        .with_reader(|c| {
            assert_eq!(
                c.query_row(
                    "SELECT COUNT(*) FROM usage_events WHERE agent='workbuddy'",
                    [],
                    |r| r.get::<_, i64>(0)
                )?,
                2
            );
            assert_eq!(
                c.query_row(
                    "SELECT COUNT(*) FROM tool_events WHERE agent='workbuddy'",
                    [],
                    |r| r.get::<_, i64>(0)
                )?,
                1
            );
            assert_eq!(
                c.query_row(
                    "SELECT COUNT(*) FROM activity_segments WHERE session_id LIKE 'workbuddy:%'",
                    [],
                    |r| r.get::<_, i64>(0)
                )?,
                0
            );
            assert_eq!(
                c.query_row(
                    "SELECT COUNT(*) FROM sessions WHERE agent='codex'",
                    [],
                    |r| r.get::<_, i64>(0)
                )?,
                0
            );
            Ok(())
        })
        .unwrap();
    assert_eq!(
        source
            .plan(&store.source_checkpoints().unwrap())
            .unwrap()
            .changes
            .len(),
        0
    );
    let exporter = UsageExporter::new(query.clone());
    for scope in [
        ExportScope::Dashboard,
        ExportScope::Sessions,
        ExportScope::Workspaces,
    ] {
        let output = root.join("workbuddy-export.json");
        exporter
            .export_to_path(
                &ExportRequest {
                    scope,
                    format: ExportFormat::Json,
                    privacy: ExportPrivacy::FullPath,
                    filters: filters.clone(),
                },
                output.to_str().unwrap(),
            )
            .unwrap();
        let exported = fs::read_to_string(output).unwrap();
        assert!(exported.contains("\"activeMs\": null"));
        assert!(!exported.contains(SECRET));
    }
    fs::create_dir_all(root.join("opencode")).unwrap();
    let other = opencode_db(&root.join("opencode/opencode.db"), time, false);
    let mut timed = oc_message(time, 100);
    timed["modelID"] = json!("deepseek-v4.1-flash");
    timed["providerID"] = json!("deepseek");
    timed["tokens"]["cache"]["write"] = json!(0);
    other
        .execute(
            "INSERT INTO message VALUES ('oc-message','same-id',?1,?1,?2)",
            params![time, timed.to_string()],
        )
        .unwrap();
    indexer.sync(SyncMode::Incremental).unwrap();
    let mut mixed = list(None);
    mixed.filters.model = Some("deepseek-v4.1-flash".into());
    assert_eq!(
        query.models(&mixed).unwrap().items[0].average_tokens_per_second,
        Some(105.0)
    );
    let mut file = fs::OpenOptions::new().append(true).open(&copy).unwrap();
    write!(file, "{{\"type\":").unwrap();
    let partial = indexer.sync(SyncMode::Incremental).unwrap();
    assert_eq!(partial.parse_failures, 0);
    assert_eq!(
        query.dashboard(&filters).unwrap().hero.real_total_tokens,
        2200
    );
    writeln!(file, "\"message\",\"id\":\"empty\"}}").unwrap();
    writeln!(file, "{}", sample("c", time + 90_000)).unwrap();
    indexer.sync(SyncMode::Incremental).unwrap();
    assert_eq!(
        query.dashboard(&filters).unwrap().hero.real_total_tokens,
        3300
    );
    // A corrupted complete line rolls back this source instead of erasing its last snapshot.
    writeln!(file, "not-json").unwrap();
    indexer.sync(SyncMode::Incremental).unwrap();
    assert_eq!(
        query.dashboard(&filters).unwrap().hero.real_total_tokens,
        3300
    );
}

#[test]
fn codebuddy_cli_transcripts_share_workbuddy_identity_with_desktop_copies() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let time = Utc::now().timestamp_millis() - 60_000;
    let timestamp = chrono::DateTime::from_timestamp_millis(time)
        .unwrap()
        .to_rfc3339();
    let request = json!({
        "id":"assistant-line", "sessionId":"cli-session", "timestamp":timestamp,
        "type":"message", "status":"completed", "cwd":"C:/work/shared",
        "providerData":{"messageId":"native-response", "requestModelId":"deepseek-v4.1-flash", "usage":{
            "inputTokens":1000, "outputTokens":200, "inputTokensDetails":[{"cached_tokens":300}],
            "outputTokensDetails":[{"reasoning_tokens":50}]
        }}, "message":{"role":"assistant","model":null,"content":SECRET}
    });
    let cli = root.join("codebuddy-cli/projects/project/cli-session.jsonl");
    write_lines(&cli, std::slice::from_ref(&request));
    write_lines(
        &root.join("workbuddy/projects/copied/cli-session.jsonl"),
        &[request],
    );
    write_lines(
        &root.join("codebuddy-cli/traces/1/trace_test.jsonl"),
        &[json!({
            "type":"trace_end", "summary":{"totalTokens":99_999}
        })],
    );
    let (store, source, indexer) = setup(root);
    let settings = source.settings().describe().unwrap();
    let workbuddy = settings
        .iter()
        .find(|setting| setting.agent == "workbuddy")
        .unwrap();
    assert!(
        workbuddy
            .roots
            .iter()
            .any(|root| root.setting_key == "workbuddyCli" && root.available)
    );
    let sync = indexer.sync(SyncMode::Initial).unwrap();
    assert_eq!((sync.files_total, sync.file_errors), (1, 0));
    let query = UsageQuery::new(store.clone(), chrono_tz::UTC);
    let list = list(Some("workbuddy"));
    let rows = query.sessions(&list).unwrap();
    assert_eq!(rows.total, 1);
    assert_eq!(rows.items[0].id, "workbuddy:cli-session");
    assert_eq!(rows.items[0].total_tokens, 1200);
    assert_eq!(rows.items[0].cached_input_tokens, 300);
    assert_eq!(rows.items[0].tokens_per_second, None);
    assert_eq!(
        query.models(&list).unwrap().items[0].model,
        "deepseek-v4.1-flash"
    );
    assert_eq!(indexer.sync(SyncMode::Incremental).unwrap().files_total, 0);
}

#[test]
fn retired_proxy_settings_and_logs_do_not_affect_native_import_or_legacy_summaries() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let timestamp = (Utc::now() - chrono::Duration::minutes(1)).to_rfc3339();
    let (store, source, indexer) = setup(root);
    let log_root = root.join("proxy/logs");
    let mut saved_settings = serde_json::to_value(source.settings()).unwrap();
    saved_settings["workbuddyProxy"] = json!(log_root);
    let settings: SourceSettings = serde_json::from_value(saved_settings).unwrap();
    assert!(
        serde_json::to_value(&settings)
            .unwrap()
            .get("workbuddyProxy")
            .is_none()
    );
    assert_eq!(
        settings
            .describe()
            .unwrap()
            .iter()
            .find(|s| s.agent == "workbuddy")
            .unwrap()
            .roots
            .len(),
        3
    );
    source.configure(settings);
    write_lines(
        &log_root.join("proxy-20261106-010101.stdout.log"),
        &[
            json!({"event":"completion","request_id":"retired","time":timestamp,
                 "input_tokens":999999,"output_tokens":999999}),
        ],
    );
    write_lines(
        &root.join("codebuddy-cli/projects/project/cli-session.jsonl"),
        &[json!({
            "id":"native", "timestamp":timestamp, "type":"message", "status":"completed",
            "cwd":"C:/work/shared", "providerData":{"messageId":"native","requestModelId":"test-model",
            "usage":{"inputTokens":1000,"outputTokens":200,"inputTokensDetails":[{"cached_tokens":300}]}},
            "message":{"role":"assistant","content":SECRET}
        })],
    );
    let initial = indexer.sync(SyncMode::Initial).unwrap();
    assert_eq!((initial.files_total, initial.file_errors), (1, 0));

    // Existing 2.3.1 history is a permanent summary after its source is retired.
    store.with_writer(|transaction| {
        transaction.execute(
            "INSERT INTO source_files(source_key,relative_path,source_kind,session_id,parser_version,status)
             VALUES ('workbuddy:proxy:retired','proxy/retired.stdout.log','session','workbuddy:proxy:retired',11,'ready')", [],
        )?;
        transaction.execute(
            "INSERT INTO sessions(id,source_file_id,workspace_id,synthetic_title,parser_version,updated_at_ms,
                model_provider,latest_model_raw,primary_model_raw,input_tokens,fresh_input_tokens,total_tokens)
             SELECT 'workbuddy:proxy:retired',
                (SELECT id FROM source_files WHERE source_key='workbuddy:proxy:retired'),
                workspace_id,'Legacy history',11,0,'workbuddy','test-model','test-model',500,500,500
             FROM sessions WHERE id='workbuddy:cli-session'", [],
        )?;
        transaction.execute(
            "INSERT INTO session_daily_usage(session_id,local_date,timezone_id,day_start_utc_ms,day_end_utc_ms,
                workspace_id,model_provider,model_raw,pricing_model_id,archived,input_tokens,fresh_input_tokens,
                total_tokens,unpriced_event_count,last_activity_at_ms)
             SELECT 'workbuddy:proxy:retired',local_date,timezone_id,day_start_utc_ms,day_end_utc_ms,
                workspace_id,model_provider,model_raw,pricing_model_id,archived,500,500,500,1,last_activity_at_ms
             FROM session_daily_usage WHERE session_id='workbuddy:cli-session'", [],
        )?;
        transaction.execute(
            "INSERT INTO usage_events(event_key,source_file_id,session_id,byte_offset,event_ordinal,
                occurred_at_ms,local_date,timezone_id,day_start_utc_ms,day_end_utc_ms,model_provider,model_raw,
                input_tokens,fresh_input_tokens,cached_input_tokens,output_tokens,reasoning_tokens,total_tokens)
             SELECT 'retired-history',(SELECT id FROM source_files WHERE source_key='workbuddy:proxy:retired'),
                'workbuddy:proxy:retired',1,1,occurred_at_ms,local_date,timezone_id,day_start_utc_ms,day_end_utc_ms,
                model_provider,model_raw,500,500,0,0,0,500 FROM usage_events WHERE session_id='workbuddy:cli-session'", [],
        )?;
        Ok(())
    }).unwrap();
    for _ in 0..2 {
        assert_eq!(indexer.sync(SyncMode::Incremental).unwrap().file_errors, 0);
        let query = UsageQuery::new(store.clone(), chrono_tz::UTC);
        for preset in [RangePreset::Last24Hours, RangePreset::Last7Days] {
            let mut selected = filters(Some("workbuddy"));
            selected.range.preset = preset;
            let snapshot = query.dashboard(&selected).unwrap();
            assert_eq!(snapshot.hero.real_total_tokens, 1700);
            assert_eq!(snapshot.hero.cache_hit_rate, Some(0.3));
        }
        let models = query.models(&list(Some("workbuddy"))).unwrap();
        assert_eq!(models.items[0].total_tokens, 1700);
        assert_eq!(models.items[0].cache_hit_rate, Some(0.3));
    }
}
