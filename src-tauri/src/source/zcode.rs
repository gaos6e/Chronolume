//! Z Code's usage ledger is authoritative; transcripts and turn totals must not
//! be added to it. Project only structured statistics, never message/raw payloads.
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use rusqlite::{Connection, OpenFlags};
use serde::Serialize;

use super::normalized::{Message, Snapshot, Tokens, check_cancel};
use super::{
    ChangeAction, FirstTokenLatency, ParsedRecord, SourceChange, SourceCursor, SourceKind,
    StreamOutcome,
};
use crate::error::{AppError, AppResult};
use crate::pricing::canonical_model_name;

pub(super) fn database_path(root: &Path) -> PathBuf {
    if root.is_file() {
        root.to_path_buf()
    } else {
        root.join("cli/db/db.sqlite")
    }
}

fn open(path: &Path) -> AppResult<Connection> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| AppError::new("zcode_database_unavailable", "无法只读打开 Z Code 数据库"))?;
    connection.busy_timeout(std::time::Duration::from_secs(2))?;
    connection.pragma_update(None, "query_only", true)?;
    Ok(connection)
}

// The usage ledger predates optional cache and timing fields. Accept additive
// changes and known legacy layouts, but reject a missing core table or column.
fn projection(
    connection: &Connection,
    table: &str,
    fields: &[(&str, Option<&str>)],
) -> AppResult<String> {
    let mut statement = connection.prepare("SELECT name FROM pragma_table_info(?1)")?;
    let columns = statement
        .query_map([table], |row| row.get::<_, String>(0))?
        .collect::<Result<HashSet<_>, _>>()?;
    if columns.is_empty() {
        return Err(unsupported());
    }
    fields
        .iter()
        .map(|(name, fallback)| {
            if columns.contains(*name) {
                Ok((*name).to_string())
            } else {
                fallback.map(str::to_owned).ok_or_else(unsupported)
            }
        })
        .collect::<AppResult<Vec<_>>>()
        .map(|fields| fields.join(","))
}

fn unsupported() -> AppError {
    AppError::new("unsupported_zcode_schema", "不支持的 Z Code 用量数据结构")
}

struct Schema {
    session: String,
    request: String,
    tool: Option<String>,
    turn: Option<String>,
}

impl Schema {
    fn read(connection: &Connection) -> AppResult<Self> {
        let optional_table = |table, fields: &[(&str, Option<&str>)]| {
            let exists: bool = connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                [table],
                |row| row.get(0),
            )?;
            exists
                .then(|| projection(connection, table, fields))
                .transpose()
        };
        Ok(Self {
            session: projection(
                connection,
                "session",
                &[
                    ("id", None),
                    ("directory", None),
                    ("parent_id", Some("NULL")),
                    ("time_created", Some("NULL")),
                    ("time_updated", Some("0")),
                    ("time_archived", Some("NULL")),
                    ("time_deleted", Some("NULL")),
                ],
            )?,
            request: projection(
                connection,
                "model_usage",
                &[
                    ("id", None),
                    ("session_id", None),
                    ("started_at", None),
                    ("model_id", None),
                    ("status", None),
                    ("input_tokens", None),
                    ("output_tokens", None),
                    ("provider_id", Some("'unknown'")),
                    ("cache_read_input_tokens", Some("0")),
                    ("cache_creation_input_tokens", Some("0")),
                    ("reasoning_tokens", Some("0")),
                    ("completed_at", Some("NULL")),
                    ("duration_ms", Some("NULL")),
                    ("turn_id", Some("NULL")),
                ],
            )?,
            tool: optional_table(
                "tool_usage",
                &[
                    ("tool_call_id", None),
                    ("tool_name", None),
                    ("started_at", None),
                    ("status", None),
                    ("completed_at", Some("NULL")),
                    ("duration_ms", Some("NULL")),
                    ("session_id", None),
                ],
            )?,
            turn: optional_table(
                "turn_usage",
                &[
                    ("turn_id", None),
                    ("started_at", None),
                    ("status", None),
                    ("first_token_at", Some("NULL")),
                    ("time_to_first_token_ms", Some("NULL")),
                    ("session_id", None),
                ],
            )?,
        })
    }
}

#[derive(Serialize)]
struct Session {
    id: String,
    directory: String,
    parent: Option<String>,
    created: Option<i64>,
    updated: i64,
    archived: Option<i64>,
    deleted: Option<i64>,
}

#[derive(Serialize)]
struct Request {
    id: String,
    started: i64,
    model: String,
    status: String,
    input: u64,
    output: u64,
    provider: Option<String>,
    cached: u64,
    write: u64,
    reasoning: u64,
    completed: Option<i64>,
    duration: Option<i64>,
    turn: Option<String>,
}

#[derive(Serialize)]
struct Tool {
    id: String,
    name: String,
    started: i64,
    status: String,
    completed: Option<i64>,
    duration: Option<i64>,
}

#[derive(Serialize)]
struct Turn {
    id: String,
    started: i64,
    status: String,
    first_token: Option<i64>,
    latency: Option<i64>,
}

#[derive(Serialize)]
struct Ledger {
    session: Session,
    requests: Vec<Request>,
    tools: Vec<Tool>,
    turns: Vec<Turn>,
}

impl Ledger {
    fn read(connection: &Connection, schema: &Schema, session: Session) -> AppResult<Self> {
        let mut statement = connection.prepare(&format!(
            "SELECT {} FROM model_usage WHERE session_id=?1 ORDER BY started_at,id",
            schema.request
        ))?;
        let requests = statement
            .query_map([&session.id], |row| {
                Ok(Request {
                    id: row.get(0)?,
                    started: row.get(2)?,
                    model: row.get(3)?,
                    status: row.get(4)?,
                    input: token_count(row, 5)?,
                    output: token_count(row, 6)?,
                    provider: row.get(7)?,
                    cached: token_count(row, 8)?,
                    write: token_count(row, 9)?,
                    reasoning: token_count(row, 10)?,
                    completed: row.get(11)?,
                    duration: row.get(12)?,
                    turn: row.get(13)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let tools = if let Some(fields) = &schema.tool {
            let mut statement = connection.prepare(&format!(
                "SELECT {fields} FROM tool_usage WHERE session_id=?1 ORDER BY started_at,tool_call_id"
            ))?;
            statement
                .query_map([&session.id], |row| {
                    Ok(Tool {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        started: row.get(2)?,
                        status: row.get(3)?,
                        completed: row.get(4)?,
                        duration: row.get(5)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?
        } else {
            Vec::new()
        };
        let turns = if let Some(fields) = &schema.turn {
            let mut statement = connection.prepare(&format!(
                "SELECT {fields} FROM turn_usage WHERE session_id=?1 ORDER BY started_at,turn_id"
            ))?;
            statement
                .query_map([&session.id], |row| {
                    Ok(Turn {
                        id: row.get(0)?,
                        started: row.get(1)?,
                        status: row.get(2)?,
                        first_token: row.get(3)?,
                        latency: row.get(4)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?
        } else {
            Vec::new()
        };
        Ok(Self {
            session,
            requests,
            tools,
            turns,
        })
    }
}

fn session(row: &rusqlite::Row<'_>) -> rusqlite::Result<Session> {
    Ok(Session {
        id: row.get(0)?,
        directory: row.get(1)?,
        parent: row.get(2)?,
        created: row.get(3)?,
        updated: row.get(4)?,
        archived: row.get(5)?,
        deleted: row.get(6)?,
    })
}

fn token_count(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(index)?;
    value
        .try_into()
        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, value))
}

pub(super) fn discover(root: &Path) -> AppResult<Vec<SourceChange>> {
    let path = database_path(root);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let mut connection = open(&path)?;
    let transaction = connection.transaction()?;
    let schema = Schema::read(&transaction)?;
    let mut statement = transaction.prepare(&format!(
        "SELECT {} FROM session ORDER BY id",
        schema.session
    ))?;
    let mut changes = Vec::new();
    for row in statement.query_map([], session)? {
        let ledger = Ledger::read(&transaction, &schema, row?)?;
        let structured = serde_json::to_vec(&ledger).map_err(|_| AppError::database())?;
        // Hash every projected field: WAL writes and equal-length, in-place
        // corrections need detection even when row counts/timestamps stay equal.
        changes.push(SourceChange {
            source_key: format!("zcode:{}", ledger.session.id),
            session_id: Some(format!("zcode:{}", ledger.session.id)),
            path: path.clone(),
            relative_path: format!("sessions/{}", ledger.session.id),
            kind: SourceKind::Session,
            action: ChangeAction::Replay,
            file_size: structured.len() as u64,
            mtime_ns: ledger.session.updated.max(0) as u128 * 1_000_000,
            prefix_hash: blake3::hash(&structured).to_hex().to_string(),
            cursor: SourceCursor::default(),
        });
    }
    Ok(changes)
}

fn finished(status: &str) -> bool {
    matches!(status, "completed" | "error" | "cancelled")
}

fn interval(start: i64, end: Option<i64>, duration: Option<i64>) -> Option<(i64, i64)> {
    let end = end.or_else(|| {
        duration
            .filter(|d| *d >= 0)
            .and_then(|d| start.checked_add(d))
    })?;
    (start >= 0 && end > start).then_some((start, end))
}

pub(super) fn stream(
    change: &SourceChange,
    sink: &mut dyn FnMut(ParsedRecord) -> AppResult<()>,
    cancel: &AtomicBool,
) -> AppResult<StreamOutcome> {
    check_cancel(cancel)?;
    let id = change
        .source_key
        .strip_prefix("zcode:")
        .ok_or_else(unsupported)?;
    let mut connection = open(&change.path)?;
    let transaction = connection.transaction()?;
    let schema = Schema::read(&transaction)?;
    let metadata = transaction.query_row(
        &format!("SELECT {} FROM session WHERE id=?1", schema.session),
        [id],
        session,
    )?;
    let ledger = Ledger::read(&transaction, &schema, metadata)?;
    check_cancel(cancel)?;
    let mut snapshot = Snapshot {
        cwd: Some(ledger.session.directory),
        parent: ledger.session.parent.map(|id| format!("zcode:{id}")),
        created: ledger.session.created,
        archived: ledger.session.archived.is_some() || ledger.session.deleted.is_some(),
        bytes_read: serde_json::to_vec(&(&ledger.requests, &ledger.tools, &ledger.turns))
            .map_err(|_| AppError::database())?
            .len() as u64,
        incomplete_tail: ledger.requests.iter().any(|r| r.status == "running")
            || ledger.tools.iter().any(|t| t.status == "running")
            || ledger.turns.iter().any(|t| t.status == "running"),
        ..Snapshot::default()
    };
    for request in &ledger.requests {
        check_cancel(cancel)?;
        if !finished(&request.status) {
            continue;
        }
        // Input is inclusive of cache buckets; reasoning is a subset of output.
        let cached = request.cached.min(request.input);
        let write = request.write.min(request.input - cached);
        snapshot.insert(Message {
            id: request.id.clone(),
            timestamp: request.started,
            provider: request
                .provider
                .clone()
                .filter(|p| !p.is_empty())
                .unwrap_or_else(|| "unknown".into()),
            model: request.model.clone(),
            tokens: Some(Tokens {
                fresh: request.input - cached - write,
                cached,
                write,
                output: request.output,
                reasoning: request.reasoning.min(request.output),
                ..Tokens::default()
            }),
            reported_cost: None,
            tools: BTreeMap::new(),
            interval: interval(request.started, request.completed, request.duration),
        });
    }
    for tool in ledger.tools {
        check_cancel(cancel)?;
        if !finished(&tool.status) {
            continue;
        }
        snapshot.insert(Message {
            id: format!("tool:{}", tool.id),
            timestamp: tool.started,
            provider: "unknown".into(),
            model: "unknown".into(),
            tokens: None,
            reported_cost: None,
            tools: BTreeMap::from([(tool.id, tool.name)]),
            interval: interval(tool.started, tool.completed, tool.duration),
        });
    }
    let mut outcome = snapshot.emit(change, sink, cancel)?;
    // Native turn latency is one sample per turn, not one per model request.
    for (index, turn) in ledger.turns.iter().enumerate() {
        check_cancel(cancel)?;
        if !finished(&turn.status) {
            continue;
        }
        let latency = turn
            .latency
            .or_else(|| turn.first_token.and_then(|t| t.checked_sub(turn.started)));
        let Some(latency_ms) = latency.filter(|ms| *ms >= 0) else {
            continue;
        };
        let Some(first_token) = turn
            .first_token
            .or_else(|| turn.started.checked_add(latency_ms))
        else {
            continue;
        };
        let Some(request) = ledger.requests.iter().find(|r| {
            r.turn.as_deref() == Some(&turn.id)
                && finished(&r.status)
                && r.started <= first_token
                && r.completed.is_none_or(|end| end >= first_token)
        }) else {
            continue;
        };
        sink(ParsedRecord::FirstTokenLatency(FirstTokenLatency {
            byte_offset: index as u64 + 1,
            turn_id: Some(format!("zcode:{id}:{}", turn.id)),
            occurred_at_ms: first_token,
            model_provider: request.provider.clone().unwrap_or_else(|| "unknown".into()),
            model_raw: canonical_model_name(&request.model),
            latency_ms,
        }))?;
        outcome.records_emitted += 1;
    }
    Ok(outcome)
}
