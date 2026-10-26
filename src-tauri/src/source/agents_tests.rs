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
fn agents_share_workspaces_keep_identity_and_never_persist_content() {
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
    assert_eq!(query.workspaces(&list(None)).unwrap().total, 1);
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
        .delete_model_price("deepseek-official", "deepseek-v4-pro")
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
