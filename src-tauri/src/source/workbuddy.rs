//! WorkBuddy desktop transcripts. Database rows supply metadata, never token or
//! credit totals. Both desktop variants share one native session namespace.
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use rusqlite::{Connection, OpenFlags};
use serde::Deserialize;
use serde_json::Value;
use walkdir::WalkDir;

use super::normalized::{Message, Snapshot, Tokens, check_cancel, timestamp};
use super::{
    ChangeAction, ParsedRecord, SourceChange, SourceCursor, SourceKind, StreamOutcome, modified_ns,
    prefix_hash,
};
use crate::error::{AppError, AppResult};

#[derive(Default, Clone)]
struct Metadata {
    cwd: Option<String>,
    created: Option<i64>,
    updated: i64,
    archived: bool,
}

fn metadata(root: &Path) -> AppResult<BTreeMap<String, Metadata>> {
    let path = root.join("workbuddy.db");
    if !path.is_file() {
        return Ok(BTreeMap::new());
    }
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.busy_timeout(std::time::Duration::from_secs(2))?;
    connection.pragma_update(None, "query_only", true)?;
    let mut statement = connection
        .prepare("SELECT id, cwd, created_at, updated_at, status, deleted_at FROM sessions")?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            Metadata {
                cwd: row.get(1)?,
                created: row.get(2)?,
                updated: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                archived: row.get::<_, Option<String>>(4)?.as_deref() == Some("archived")
                    || row.get::<_, Option<i64>>(5)?.is_some(),
            },
        ))
    })?;
    rows.collect::<Result<_, _>>().map_err(AppError::from)
}

fn files(roots: &[PathBuf]) -> AppResult<BTreeMap<String, Vec<PathBuf>>> {
    let mut files: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for root in roots {
        let projects = root.join("projects");
        if !projects.is_dir() {
            continue;
        }
        for entry in WalkDir::new(projects).follow_links(false) {
            let entry = entry.map_err(|_| AppError::filesystem())?;
            if !entry.file_type().is_file()
                || entry.path().extension().and_then(|s| s.to_str()) != Some("jsonl")
            {
                continue;
            }
            let path = fs::canonicalize(entry.path())?;
            if !seen.insert(path.clone()) {
                continue;
            }
            let id = path
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(AppError::filesystem)?
                .to_owned();
            files.entry(id).or_default().push(path);
        }
    }
    for paths in files.values_mut() {
        paths.sort();
    }
    Ok(files)
}

fn all_metadata(roots: &[PathBuf]) -> AppResult<BTreeMap<String, Metadata>> {
    let mut merged: BTreeMap<String, Metadata> = BTreeMap::new();
    for root in roots {
        for (id, next) in metadata(root)? {
            if merged
                .get(&id)
                .is_none_or(|old| next.updated >= old.updated)
            {
                merged.insert(id, next);
            }
        }
    }
    Ok(merged)
}

pub(super) fn discover(roots: &[PathBuf]) -> AppResult<Vec<SourceChange>> {
    let metadata = all_metadata(roots)?;
    let mut changes = Vec::new();
    for (id, paths) in files(roots)? {
        let mut bytes = 0;
        let mut stamp = 0;
        let mut fingerprint = String::new();
        for path in &paths {
            let info = fs::metadata(path)?;
            bytes += info.len();
            stamp = stamp.max(modified_ns(&info));
            fingerprint.push_str(&format!(
                "{}:{}:{}:{};",
                path.display(),
                info.len(),
                modified_ns(&info),
                prefix_hash(path)?
            ));
        }
        if let Some(meta) = metadata.get(&id) {
            fingerprint.push_str(&format!(
                "{:?}:{:?}:{}:{}",
                meta.cwd, meta.created, meta.updated, meta.archived
            ));
        }
        changes.push(SourceChange {
            source_key: format!("workbuddy:{id}"),
            session_id: Some(format!("workbuddy:{id}")),
            path: paths[0].clone(),
            relative_path: format!("projects/{id}.jsonl"),
            kind: SourceKind::Session,
            action: ChangeAction::Replay,
            file_size: bytes,
            mtime_ns: stamp,
            prefix_hash: blake3::hash(fingerprint.as_bytes()).to_hex().to_string(),
            cursor: SourceCursor::default(),
        });
    }
    Ok(changes)
}

// Whitelist analytics fields. Conversation bodies, tool inputs/results and
// session titles are skipped by serde rather than entering the shared snapshot.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    id: Option<String>,
    timestamp: Option<Value>,
    cwd: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    status: Option<String>,
    name: Option<String>,
    call_id: Option<String>,
    provider_data: Option<ProviderData>,
    message: Option<LegacyMessage>,
}
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ProviderData {
    message_id: Option<String>,
    conversation_request_id: Option<String>,
    model: Option<String>,
    request_model_id: Option<String>,
    provider: Option<String>,
    usage: Option<Value>,
    raw_usage: Option<Value>,
}
#[derive(Deserialize)]
struct LegacyMessage {
    usage: Option<Value>,
}

fn field(value: Option<&Value>, names: &[&str]) -> Option<u64> {
    names
        .iter()
        .find_map(|key| value?.get(key)?.as_u64().filter(|n| *n <= i64::MAX as u64))
}
fn detail(value: Option<&Value>, field_name: &str, key: &str) -> Option<u64> {
    let value = value?.get(field_name)?;
    if let Some(array) = value.as_array() {
        let mut total = 0_u64;
        let mut found = false;
        for item in array {
            if let Some(n) = field(Some(item), &[key]) {
                total = total.saturating_add(n);
                found = true;
            }
        }
        found.then_some(total)
    } else {
        field(Some(value), &[key])
    }
}
fn tokens(data: &ProviderData, legacy: Option<&Value>) -> Option<Tokens> {
    let usage = data.usage.as_ref();
    let raw = data.raw_usage.as_ref();
    let input = field(usage, &["inputTokens"])
        .or_else(|| field(raw, &["prompt_tokens", "input_tokens"]))
        .or_else(|| field(legacy, &["input_tokens"]))?;
    let output = field(usage, &["outputTokens"])
        .or_else(|| field(raw, &["completion_tokens", "output_tokens"]))
        .or_else(|| field(legacy, &["output_tokens"]))?;
    let cached = detail(usage, "inputTokensDetails", "cached_tokens")
        .or_else(|| {
            field(
                raw,
                &[
                    "prompt_cache_hit_tokens",
                    "cache_read_input_tokens",
                    "cached_tokens",
                ],
            )
        })
        .or_else(|| detail(raw, "prompt_tokens_details", "cached_tokens"))
        .or_else(|| field(legacy, &["cache_read_input_tokens"]))
        .unwrap_or(0);
    let write = field(
        raw,
        &["prompt_cache_write_tokens", "cache_creation_input_tokens"],
    )
    .or_else(|| field(legacy, &["cache_creation_input_tokens"]))
    .unwrap_or(0);
    let reasoning = detail(usage, "outputTokensDetails", "reasoning_tokens")
        .or_else(|| detail(raw, "completion_tokens_details", "reasoning_tokens"))
        .or_else(|| field(raw, &["completion_thinking_tokens"]))
        .unwrap_or(0);
    if cached.saturating_add(write) > input || reasoning > output {
        return None;
    }
    Some(Tokens {
        fresh: input - cached - write,
        cached,
        write,
        write_1h: 0,
        output,
        reasoning,
    })
}

pub(super) fn stream(
    change: &SourceChange,
    roots: &[PathBuf],
    sink: &mut dyn FnMut(ParsedRecord) -> AppResult<()>,
    cancel: &AtomicBool,
) -> AppResult<StreamOutcome> {
    let id = change
        .session_id
        .as_deref()
        .and_then(|s| s.strip_prefix("workbuddy:"))
        .ok_or_else(AppError::filesystem)?;
    let meta = all_metadata(roots)?.remove(id).unwrap_or_default();
    let mut snapshot = Snapshot {
        cwd: meta.cwd,
        created: meta.created,
        archived: meta.archived,
        ..Snapshot::default()
    };
    let paths = files(roots)?.remove(id).ok_or_else(AppError::filesystem)?;
    let mut requests: BTreeMap<String, Message> = BTreeMap::new();
    let mut tools: BTreeMap<String, Message> = BTreeMap::new();
    let mut rejected = BTreeSet::new();
    let mut rejected_requests = BTreeSet::new();
    for path in paths {
        let mut reader = BufReader::new(File::open(path)?);
        let mut line = Vec::new();
        loop {
            check_cancel(cancel)?;
            line.clear();
            let bytes = reader.read_until(b'\n', &mut line)?;
            if bytes == 0 {
                break;
            }
            snapshot.bytes_read += bytes as u64;
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            let entry: Entry = match serde_json::from_slice(&line) {
                Ok(entry) => entry,
                Err(_) if line.last() != Some(&b'\n') => {
                    snapshot.incomplete_tail = true;
                    break;
                }
                Err(_) => {
                    return Err(AppError::new(
                        "workbuddy_transcript_invalid",
                        "WorkBuddy 会话记录格式损坏",
                    ));
                }
            };
            if snapshot.cwd.is_none() {
                snapshot.cwd = entry.cwd;
            }
            let Some(time) = timestamp(entry.timestamp.as_ref()) else {
                continue;
            };
            let data = entry.provider_data.unwrap_or_default();
            let model = data
                .request_model_id
                .or(data.model)
                .unwrap_or_else(|| "unknown".into());
            let provider = data
                .provider
                .as_deref()
                .filter(|p| !p.is_empty())
                .unwrap_or("workbuddy")
                .to_owned();
            let request_id = data
                .message_id
                .as_ref()
                .or(data.conversation_request_id.as_ref())
                .or(entry.id.as_ref())
                .cloned();
            let invalid = entry.status.as_deref().is_some_and(|s| {
                matches!(
                    s,
                    "incomplete"
                        | "failed"
                        | "error"
                        | "cancelled"
                        | "canceled"
                        | "aborted"
                        | "interrupted"
                )
            });
            if let Some(call) = &entry.call_id {
                if invalid {
                    rejected.insert(call.clone());
                }
                if entry.kind.as_deref() == Some("function_call") && !invalid {
                    if let Some(name) = entry.name {
                        tools.entry(call.clone()).or_insert(Message {
                            id: format!("tool:{call}"),
                            timestamp: time,
                            provider: provider.clone(),
                            model: model.clone(),
                            tokens: None,
                            reported_cost: None,
                            tools: BTreeMap::from([(call.clone(), name)]),
                            interval: None,
                        });
                    }
                }
            }
            if invalid {
                if let Some(id) = request_id {
                    rejected_requests.insert(id);
                }
                continue;
            }
            let legacy = entry.message.as_ref().and_then(|m| m.usage.as_ref());
            // Each call carries a single complete native usage sample. Repeated
            // function/message representations and copied files share its ID.
            let token_data = tokens(
                &ProviderData {
                    model: None,
                    request_model_id: None,
                    ..data
                },
                legacy,
            );
            if let (Some(id), Some(tokens)) = (request_id, token_data) {
                let next = Message {
                    id: id.clone(),
                    timestamp: time,
                    provider,
                    model,
                    tokens: Some(tokens),
                    reported_cost: None,
                    tools: BTreeMap::new(),
                    interval: None,
                };
                requests
                    .entry(id)
                    .and_modify(|old| old.merge(next.clone()))
                    .or_insert(next);
            }
        }
    }
    for (id, message) in requests {
        if !rejected_requests.contains(&id) {
            snapshot.insert(message);
        }
    }
    for (id, message) in tools {
        if !rejected.contains(&id) {
            snapshot.insert(message);
        }
    }
    snapshot.emit(change, sink, cancel)
}
