use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde_json::{Value, json};
use walkdir::WalkDir;

use super::normalized::{Message, Snapshot, Tokens, check_cancel, cost, number, timestamp};
use super::{
    ChangeAction, ParsedRecord, SourceChange, SourceCursor, SourceKind, StreamOutcome, modified_ns,
};
use crate::error::{AppError, AppResult};

pub(super) fn databases(root: &Path) -> AppResult<Vec<PathBuf>> {
    if root.is_file() {
        return Ok(vec![root.to_path_buf()]);
    }
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut paths = Vec::new();
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if path.is_file() && name.starts_with("opencode") && name.ends_with(".db") {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

fn open(path: &Path) -> AppResult<Connection> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| {
        AppError::new(
            "opencode_database_unavailable",
            "无法只读打开 OpenCode 数据库",
        )
    })?;
    connection.busy_timeout(std::time::Duration::from_secs(2))?;
    connection.pragma_update(None, "query_only", true)?;
    Ok(connection)
}

fn table_exists(connection: &Connection, table: &str) -> AppResult<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        [table],
        |row| row.get(0),
    )?)
}

fn column(connection: &Connection, table: &str, name: &str, fallback: &str) -> AppResult<String> {
    let exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info(?1) WHERE name=?2)",
        [table, name],
        |row| row.get(0),
    )?;
    Ok(if exists {
        name.to_string()
    } else {
        fallback.to_string()
    })
}

fn session_tables(connection: &Connection) -> AppResult<Vec<&'static str>> {
    let mut tables = Vec::new();
    for table in ["session", "session_v2"] {
        if table_exists(connection, table)? {
            tables.push(table);
        }
    }
    if tables.is_empty() {
        return Err(AppError::new(
            "unsupported_opencode_schema",
            "不支持的 OpenCode 数据结构",
        ));
    }
    Ok(tables)
}

#[derive(Default)]
struct Discovery {
    bytes: u64,
    stamp: u128,
    fingerprint: String,
}

pub(super) fn discover(root: &Path) -> AppResult<Vec<SourceChange>> {
    let mut sessions: BTreeMap<String, Discovery> = BTreeMap::new();
    for path in databases(root)? {
        let connection = open(&path)?;
        for session_table in session_tables(&connection)? {
            let parent = column(&connection, session_table, "parent_id", "NULL")?;
            let archive = column(&connection, session_table, "time_archived", "NULL")?;
            let mut statement = connection.prepare(&format!(
            "SELECT id, directory, time_created, time_updated, {parent}, {archive} FROM {session_table}"
        ))?;
            let rows = statement.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                ))
            })?;
            for row in rows {
                let (id, directory, created, updated, parent, archive) = row?;
                let discovery = sessions.entry(id.clone()).or_default();
                discovery.fingerprint.push_str(
                    &json!([
                        path.file_name().and_then(|s| s.to_str()),
                        session_table,
                        directory,
                        created,
                        updated,
                        parent,
                        archive
                    ])
                    .to_string(),
                );
                discovery.stamp = discovery.stamp.max(updated.max(0) as u128);
                for table in ["message", "part", "session_message"] {
                    if !table_exists(&connection, table)? {
                        continue;
                    }
                    let time = column(&connection, table, "time_updated", "time_created")?;
                    let (count, size, updated): (i64, i64, i64) = connection.query_row(
                    &format!("SELECT COUNT(*), COALESCE(SUM(length(data)), 0), COALESCE(MAX({time}), 0) FROM {table} WHERE session_id=?1"),
                    [&id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
                    discovery.bytes += size.max(0) as u64;
                    discovery
                        .fingerprint
                        .push_str(&format!("{table}:{count}:{size}:{updated};"));
                }
            }
        }
    }
    let legacy = legacy_root(root);
    let message_root = legacy.join("storage/message");
    if message_root.is_dir() {
        for entry in fs::read_dir(&message_root)? {
            let path = entry?.path();
            if !path.is_dir() {
                continue;
            }
            let Some(id) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            let discovery = sessions.entry(id.into()).or_default();
            for path in json_files(&path)? {
                add_file(discovery, &path)?;
                if let Some(message) = path.file_stem() {
                    for part in json_files(&legacy.join("storage/part").join(message))? {
                        add_file(discovery, &part)?;
                    }
                }
            }
        }
    }
    for path in json_files(&legacy.join("storage/session"))? {
        if let Some(id) = path.file_stem().and_then(|s| s.to_str()) {
            if let Some(discovery) = sessions.get_mut(id) {
                add_file(discovery, &path)?;
            }
        }
    }
    Ok(sessions
        .into_iter()
        .map(|(id, discovery)| SourceChange {
            source_key: format!("opencode:{id}"),
            session_id: Some(format!("opencode:{id}")),
            path: root.to_path_buf(),
            relative_path: format!("sessions/{id}"),
            kind: SourceKind::Session,
            action: ChangeAction::Replay,
            file_size: discovery.bytes,
            mtime_ns: discovery.stamp,
            prefix_hash: blake3::hash(discovery.fingerprint.as_bytes())
                .to_hex()
                .to_string(),
            cursor: SourceCursor::default(),
        })
        .collect())
}

fn add_file(discovery: &mut Discovery, path: &Path) -> AppResult<()> {
    let metadata = fs::metadata(path)?;
    discovery.bytes += metadata.len();
    discovery.stamp = discovery.stamp.max(modified_ns(&metadata));
    discovery.fingerprint.push_str(&format!(
        "{}:{}:{};",
        path.file_name().unwrap_or_default().to_string_lossy(),
        metadata.len(),
        modified_ns(&metadata)
    ));
    Ok(())
}

fn legacy_root(root: &Path) -> PathBuf {
    if root.is_file() {
        root.parent().unwrap_or(root).to_path_buf()
    } else {
        root.to_path_buf()
    }
}

fn json_files(root: &Path) -> AppResult<Vec<PathBuf>> {
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut paths = Vec::new();
    for entry in WalkDir::new(root).follow_links(false) {
        let entry = entry.map_err(|_| AppError::filesystem())?;
        if entry.file_type().is_file()
            && entry.path().extension().and_then(|s| s.to_str()) == Some("json")
        {
            paths.push(entry.into_path());
        }
    }
    paths.sort();
    Ok(paths)
}

fn message(id: String, data: &Value, fallback: Option<i64>) -> Option<Message> {
    let time = timestamp(data.pointer("/time/created")).or(fallback)?;
    let model = data
        .get("modelID")
        .and_then(Value::as_str)
        .or_else(|| data.pointer("/model/id").and_then(Value::as_str))
        .or_else(|| data.pointer("/model/modelID").and_then(Value::as_str))
        .unwrap_or("unknown")
        .to_string();
    let provider = data
        .get("providerID")
        .and_then(Value::as_str)
        .or_else(|| data.pointer("/model/providerID").and_then(Value::as_str))
        .or_else(|| data.pointer("/model/providerId").and_then(Value::as_str))
        .unwrap_or("unknown")
        .trim()
        .to_ascii_lowercase();
    let tokens = data.get("tokens").filter(|v| v.is_object()).map(|tokens| {
        let output = number(tokens.get("output"));
        let reasoning = number(tokens.get("reasoning"));
        // OpenCode separates generated reasoning from text output. An explicit total
        // takes precedence where a provider already includes reasoning in output.
        let fresh = number(tokens.get("input"));
        let cached = number(tokens.pointer("/cache/read"));
        let write = number(tokens.pointer("/cache/write"));
        let known = fresh
            .saturating_add(cached)
            .saturating_add(write)
            .saturating_add(output);
        let reported_total = number(tokens.get("total"));
        let extra = if reported_total > 0 {
            reported_total.saturating_sub(known).min(reasoning)
        } else {
            reasoning
        };
        Tokens {
            fresh,
            cached,
            write,
            write_1h: 0,
            output: output.saturating_add(extra),
            reasoning,
        }
    });
    let completed = timestamp(data.pointer("/time/completed"));
    Some(Message {
        id,
        timestamp: time,
        provider,
        model,
        tokens,
        reported_cost: cost(data.get("cost")),
        tools: BTreeMap::new(),
        interval: completed.filter(|end| *end >= time).map(|end| (time, end)),
    })
}

pub(super) fn stream(
    change: &SourceChange,
    sink: &mut dyn FnMut(ParsedRecord) -> AppResult<()>,
    cancel: &AtomicBool,
) -> AppResult<StreamOutcome> {
    let id = change
        .session_id
        .as_deref()
        .and_then(|id| id.strip_prefix("opencode:"))
        .unwrap_or("");
    let mut snapshot = Snapshot::default();
    let legacy = legacy_root(&change.path);
    for path in json_files(&legacy.join("storage/session"))? {
        if path.file_stem().and_then(|s| s.to_str()) != Some(id) {
            continue;
        }
        let data: Value = serde_json::from_slice(&fs::read(path)?)
            .map_err(|_| AppError::new("opencode_invalid_metadata", "OpenCode 元数据格式无效"))?;
        snapshot.cwd = data
            .get("directory")
            .and_then(Value::as_str)
            .map(str::to_string);
        snapshot.parent = data
            .get("parentID")
            .and_then(Value::as_str)
            .map(|id| format!("opencode:{id}"));
        snapshot.created = timestamp(data.pointer("/time/created"));
        snapshot.archived = data.pointer("/time/archived").is_some_and(|v| !v.is_null());
    }
    for path in json_files(&legacy.join("storage/message").join(id))? {
        check_cancel(cancel)?;
        let bytes = fs::read(&path)?;
        snapshot.bytes_read += bytes.len() as u64;
        let data: Value = match serde_json::from_slice(&bytes) {
            Ok(data) => data,
            Err(_) => {
                snapshot.parse_failures += 1;
                continue;
            }
        };
        if data.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let message_id = data
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| {
                path.file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            });
        if let Some(mut message) = message(message_id.clone(), &data, None) {
            for part in json_files(&legacy.join("storage/part").join(&message_id))? {
                let bytes = fs::read(part)?;
                snapshot.bytes_read += bytes.len() as u64;
                if let Ok(data) = serde_json::from_slice::<Value>(&bytes) {
                    if data.get("type").and_then(Value::as_str) == Some("tool") {
                        if let Some(name) = data.get("tool").and_then(Value::as_str) {
                            let key = data
                                .get("callID")
                                .or_else(|| data.get("id"))
                                .and_then(Value::as_str)
                                .unwrap_or(name);
                            message.tools.insert(key.into(), name.into());
                        }
                    }
                } else {
                    snapshot.parse_failures += 1;
                }
            }
            snapshot.insert(message);
        }
    }
    for path in databases(&change.path)? {
        check_cancel(cancel)?;
        let connection = open(&path)?;
        for session_table in session_tables(&connection)? {
            let parent = column(&connection, session_table, "parent_id", "NULL")?;
            let archive = column(&connection, session_table, "time_archived", "NULL")?;
            if let Some((directory, created, parent, archive)) = connection
            .query_row(
                &format!(
                    "SELECT directory, time_created, {parent}, {archive} FROM {session_table} WHERE id=?1"
                ),
                [id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                    ))
                },
            )
            .optional()?
        {
            snapshot.cwd = Some(directory);
            snapshot.created = Some(created);
            snapshot.parent = parent.map(|id| format!("opencode:{id}"));
            snapshot.archived = archive.is_some();
        }
        }
        for table in ["message", "session_message"] {
            if !table_exists(&connection, table)? {
                continue;
            }
            let role = if table == "session_message" {
                "type"
            } else {
                "json_extract(data, '$.role')"
            };
            // Extract the whitelist in SQLite. Conversation text and tool inputs never
            // leave the source database, including for the embedded v2 tool parts.
            let sql = format!(
                "SELECT id, time_created, json_object('modelID', json_extract(data,'$.modelID'), 'providerID', json_extract(data,'$.providerID'), 'model', json_extract(data,'$.model'), 'tokens', json_extract(data,'$.tokens'), 'time', json_extract(data,'$.time'), 'cost', json_extract(data,'$.cost')) FROM {table} WHERE session_id=?1 AND {role}='assistant' AND json_valid(data)"
            );
            let mut statement = connection.prepare(&sql)?;
            let rows = statement.query_map([id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?;
            for row in rows {
                check_cancel(cancel)?;
                let (message_id, created, data) = row?;
                snapshot.bytes_read += data.len() as u64;
                let data: Value = serde_json::from_str(&data).map_err(|_| AppError::database())?;
                if let Some(message) = message(message_id.clone(), &data, Some(created)) {
                    snapshot.messages.insert(message_id, message);
                }
            }
            if table == "session_message" {
                for content in ["content", "parts"] {
                    let sql = format!(
                        "SELECT m.id, json_extract(p.value, '$.id'), json_extract(p.value, '$.name') FROM session_message m, json_each(m.data, '$.{content}') p WHERE m.session_id=?1 AND m.type='assistant' AND json_extract(p.value,'$.type')='tool'"
                    );
                    let mut statement = connection.prepare(&sql)?;
                    let rows = statement.query_map([id], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, Option<String>>(1)?,
                            row.get::<_, Option<String>>(2)?,
                        ))
                    })?;
                    for row in rows {
                        let (message, key, name) = row?;
                        if let (Some(message), Some(name)) =
                            (snapshot.messages.get_mut(&message), name)
                        {
                            message
                                .tools
                                .insert(key.unwrap_or_else(|| name.clone()), name);
                        }
                    }
                }
            }
        }
        if table_exists(&connection, "part")? {
            let mut statement = connection.prepare("SELECT message_id, id, json_extract(data,'$.callID'), json_extract(data,'$.tool') FROM part WHERE session_id=?1 AND json_valid(data) AND json_extract(data,'$.type')='tool'")?;
            let rows = statement.query_map([id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })?;
            for row in rows {
                let (message, part, call, name) = row?;
                if let (Some(message), Some(name)) = (snapshot.messages.get_mut(&message), name) {
                    message.tools.insert(call.unwrap_or(part), name);
                }
            }
        }
    }
    // Forks copy the parent's transcript. The child's own work starts at creation.
    if snapshot.parent.is_some() {
        if let Some(created) = snapshot.created {
            snapshot
                .messages
                .retain(|_, message| message.timestamp >= created);
        }
    }
    snapshot.emit(change, sink, cancel)
}
