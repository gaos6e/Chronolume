//! Read-only analytics for DeepSeek Harness JSONL generations (raw or Zstandard).
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use serde::Deserialize;
use walkdir::WalkDir;

use super::normalized::{Message, Snapshot, Tokens, check_cancel};
use super::{
    ChangeAction, FirstTokenLatency, ParsedRecord, SourceChange, SourceCursor, SourceKind,
    StreamOutcome, modified_ns, normalize_relative_path, prefix_hash,
};
use crate::error::{AppError, AppResult};

// Explicit field selection keeps prompts, stream text, tool arguments, and credentials
// outside the analytics snapshot. Packed delta rows need only their first timestamp.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Header {
    #[serde(rename = "type")]
    kind: String,
    version: u64,
    id: String,
    created_at: i64,
    cwd: Option<String>,
    parent_session: Option<String>,
    seed_length: Option<u64>,
    is_seeded: Option<bool>,
}

#[derive(Default, Deserialize)]
struct Row {
    #[serde(rename = "type")]
    kind: String,
    seq: Option<u64>,
    seq0: Option<u64>,
    time: Option<i64>,
    time0: Option<i64>,
    #[serde(default)]
    data: Data,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Data {
    turn: Option<u64>,
    step: Option<u64>,
    usage: Option<Usage>,
    chunk: Option<Chunk>,
    #[serde(default)]
    stream: Vec<StreamRecord>,
    header: Option<RequestHeader>,
    provider: Option<String>,
    model: Option<String>,
    call_id: Option<String>,
    name: Option<String>,
    inherited: Option<bool>,
}

#[derive(Deserialize)]
struct RequestHeader {
    config: Route,
}
#[derive(Default, Deserialize)]
struct Route {
    provider: Option<String>,
    model: Option<String>,
}
#[derive(Deserialize)]
struct Chunk {
    #[serde(rename = "type")]
    kind: String,
    usage: Option<Usage>,
}
#[derive(Deserialize)]
struct StreamRecord {
    #[serde(rename = "type")]
    kind: String,
    time: Option<i64>,
    time0: Option<i64>,
    chunk: Option<Chunk>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Usage {
    input_tokens: u64,
    output_tokens: u64,
    #[serde(default)]
    cache_read_tokens: u64,
    #[serde(default)]
    cache_write_tokens: u64,
    #[serde(default)]
    reasoning_tokens: u64,
}
impl Usage {
    fn tokens(self) -> AppResult<Tokens> {
        let input = self
            .input_tokens
            .checked_add(self.cache_read_tokens)
            .and_then(|n| n.checked_add(self.cache_write_tokens));
        if input
            .and_then(|n| n.checked_add(self.output_tokens))
            .is_none_or(|n| n > i64::MAX as u64)
            || self.reasoning_tokens > self.output_tokens
        {
            return Err(corrupt());
        }
        Ok(Tokens {
            fresh: self.input_tokens,
            cached: self.cache_read_tokens,
            write: self.cache_write_tokens,
            output: self.output_tokens,
            reasoning: self.reasoning_tokens,
            ..Tokens::default()
        })
    }
}

fn corrupt() -> AppError {
    AppError::new("dsh_log_invalid", "DeepSeek Harness 会话格式或统计字段无效")
}

fn reader(path: &Path) -> AppResult<Box<dyn BufRead>> {
    let file = File::open(path)?;
    let input: Box<dyn Read> = if path.extension().is_some_and(|ext| ext == "zstd") {
        Box::new(zstd::stream::read::Decoder::new(file).map_err(|_| corrupt())?)
    } else {
        Box::new(file)
    };
    Ok(Box::new(BufReader::new(input)))
}

fn read_header(reader: &mut dyn BufRead) -> AppResult<Header> {
    let mut line = Vec::new();
    reader.read_until(b'\n', &mut line).map_err(|_| corrupt())?;
    let header: Header = serde_json::from_slice(&line).map_err(|_| corrupt())?;
    if header.kind != "session" || header.id.is_empty() || header.created_at < 0 {
        return Err(corrupt());
    }
    Ok(header)
}

fn generation(path: &Path) -> Option<u64> {
    let name = path.file_name()?.to_str()?;
    let raw = name.strip_suffix(".zstd").unwrap_or(name);
    let base = raw.strip_suffix(".jsonl")?;
    if base == "session" {
        return Some(0);
    }
    let version = base.strip_prefix("session.v")?;
    let number = version.parse::<u64>().ok()?;
    (number > 0 && version == number.to_string()).then_some(number)
}

pub(super) fn discover(root: &Path) -> AppResult<Vec<SourceChange>> {
    let sessions = if root.join("sessions").is_dir() {
        root.join("sessions")
    } else {
        root.to_path_buf()
    };
    if !sessions.is_dir() {
        return Ok(Vec::new());
    }
    let mut latest: BTreeMap<PathBuf, (u64, PathBuf)> = BTreeMap::new();
    for entry in WalkDir::new(&sessions).max_depth(3).follow_links(false) {
        let entry = entry.map_err(|_| AppError::filesystem())?;
        if !entry.file_type().is_file() {
            continue;
        }
        let Some(version) = generation(entry.path()) else {
            continue;
        };
        let directory = entry.path().parent().ok_or_else(corrupt)?.to_path_buf();
        let candidate = (version, entry.path().to_path_buf());
        if latest
            .get(&directory)
            .is_none_or(|existing| candidate > *existing)
        {
            latest.insert(directory, candidate);
        }
    }
    let mut files = Vec::new();
    for (_, path) in latest.into_values() {
        let metadata = fs::metadata(&path)?;
        let relative_path =
            normalize_relative_path(path.strip_prefix(root).map_err(|_| corrupt())?);
        // A bad file remains an individual failed source, allowing healthy sessions
        // to sync. Its synthetic key never enters sessions because stream refuses it.
        let id = reader(&path)
            .and_then(|mut input| read_header(input.as_mut()))
            .map(|h| h.id)
            .unwrap_or_else(|_| {
                format!(
                    "unreadable:{}",
                    blake3::hash(relative_path.as_bytes()).to_hex()
                )
            });
        let id = format!("dsh:{id}");
        files.push(SourceChange {
            source_key: id.clone(),
            session_id: Some(id),
            path: path.clone(),
            relative_path,
            kind: SourceKind::Session,
            action: ChangeAction::Replay,
            file_size: metadata.len(),
            mtime_ns: modified_ns(&metadata),
            prefix_hash: prefix_hash(&path)?,
            cursor: SourceCursor::default(),
        });
    }
    files.sort_by(|a, b| {
        a.source_key
            .cmp(&b.source_key)
            .then_with(|| b.file_size.cmp(&a.file_size))
            .then_with(|| a.relative_path.cmp(&b.relative_path))
    });
    files.dedup_by(|a, b| a.source_key == b.source_key);
    Ok(files)
}

struct Attempt {
    key: String,
    started: i64,
}
struct Turn {
    started: i64,
    first_token: Option<(i64, String, String)>,
}

pub(super) fn stream(
    change: &SourceChange,
    sink: &mut dyn FnMut(ParsedRecord) -> AppResult<()>,
    cancel: &AtomicBool,
) -> AppResult<StreamOutcome> {
    check_cancel(cancel)?;
    let mut input = reader(&change.path)?;
    let header = read_header(input.as_mut())?;
    if header.version > 4 {
        return Err(AppError::new(
            "dsh_format_unsupported",
            "DeepSeek Harness 会话版本尚未支持",
        ));
    }
    if change.session_id.as_deref() != Some(&format!("dsh:{}", header.id)) {
        return Err(corrupt());
    }
    let mut snapshot = Snapshot {
        cwd: header.cwd,
        parent: header.parent_session.map(|id| format!("dsh:{id}")),
        created: Some(header.created_at),
        bytes_read: change.file_size,
        ..Snapshot::default()
    };
    let mut route = Route::default();
    let mut attempts: BTreeMap<(u64, u64), Attempt> = BTreeMap::new();
    let mut turns: BTreeMap<u64, Turn> = BTreeMap::new();
    let mut saw_inherited_cut = false;
    let mut line = Vec::new();
    loop {
        check_cancel(cancel)?;
        line.clear();
        match input.read_until(b'\n', &mut line) {
            Ok(0) => break,
            Ok(_) => (),
            Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
                snapshot.incomplete_tail = true;
                break;
            }
            Err(_) => return Err(corrupt()),
        }
        if line.last() != Some(&b'\n') {
            snapshot.incomplete_tail = true;
            break;
        }
        let row: Row = serde_json::from_slice(&line).map_err(|_| corrupt())?;
        let seq = row.seq.or(row.seq0).ok_or_else(corrupt)?;
        let time = row
            .time
            .or(row.time0)
            .filter(|t| *t >= 0)
            .ok_or_else(corrupt)?;
        let data = row.data;
        if row.kind == "request/header" {
            if let Some(header) = data.header {
                route = header.config;
            }
            continue;
        }
        if row.kind == "request/context" {
            route = Route {
                provider: data.provider,
                model: data.model,
            };
            continue;
        }
        // v0-v1 carry seedLength. Newer logs carry the exact cut on the LAST
        // tagged marker; ordinary resume markers do not clear historical usage.
        if row.kind == "session/end-seed"
            && data.inherited == Some(true)
            && header.is_seeded == Some(true)
        {
            snapshot.messages.clear();
            attempts.clear();
            turns.clear();
            saw_inherited_cut = true;
            continue;
        }
        if seq < header.seed_length.unwrap_or(0) {
            continue;
        }
        let Some(turn) = data.turn else {
            continue;
        };
        if row.kind == "turn/start" {
            turns.insert(
                turn,
                Turn {
                    started: time,
                    first_token: None,
                },
            );
            continue;
        }
        let Some(step) = data.step else {
            continue;
        };
        if row.kind == "step/start" || row.kind == "llm/retry-started" {
            if let Some(previous) = attempts.get(&(turn, step)) {
                if let Some(message) = snapshot.messages.get_mut(&previous.key) {
                    message.interval =
                        (time > previous.started).then_some((previous.started, time));
                }
            }
            attempts.insert(
                (turn, step),
                Attempt {
                    key: format!("{}:{seq}", header.id),
                    started: time,
                },
            );
            turns.entry(turn).or_insert(Turn {
                started: time,
                first_token: None,
            });
            continue;
        }
        if !matches!(
            row.kind.as_str(),
            "assistant/message"
                | "assistant/attempt"
                | "assistant/chunk"
                | "tool/call"
                | "step/end"
                | "text-chunks"
                | "reasoning-chunks"
                | "tool-call-chunks"
        ) {
            continue;
        }
        let attempt = attempts.entry((turn, step)).or_insert_with(|| Attempt {
            key: format!("{}:turn:{turn}:step:{step}", header.id),
            started: time,
        });
        let provider = route.provider.as_deref().unwrap_or("unknown");
        let model = route.model.as_deref().unwrap_or("unknown");
        let message = snapshot
            .messages
            .entry(attempt.key.clone())
            .or_insert_with(|| Message {
                id: attempt.key.clone(),
                timestamp: time,
                provider: provider.into(),
                model: model.into(),
                tokens: None,
                reported_cost: None,
                tools: BTreeMap::new(),
                interval: None,
            });
        message.provider = provider.into();
        message.model = model.into();
        let final_usage = data.usage;
        let mut usage = None;
        let mut first_token = matches!(
            row.kind.as_str(),
            "text-chunks" | "reasoning-chunks" | "tool-call-chunks"
        )
        .then_some(time);
        if let Some(chunk) = data.chunk {
            if chunk.kind == "usage" {
                usage = chunk.usage;
            }
            if matches!(
                chunk.kind.as_str(),
                "text-delta" | "reasoning-delta" | "tool-call-delta"
            ) {
                first_token = Some(time);
            }
        }
        for record in data.stream {
            let mut token_time = matches!(
                record.kind.as_str(),
                "text-chunks" | "reasoning-chunks" | "tool-call-chunks"
            )
            .then_some(record.time0)
            .flatten();
            if let Some(chunk) = record.chunk {
                if chunk.kind == "usage" {
                    usage = chunk.usage;
                }
                if matches!(
                    chunk.kind.as_str(),
                    "text-delta" | "reasoning-delta" | "tool-call-delta"
                ) {
                    token_time = record.time;
                }
            }
            if let Some(token_time) = token_time {
                first_token = Some(first_token.map_or(token_time, |t| t.min(token_time)));
            }
        }
        // DSH's meter uses the LAST sample for an attempt, which may correct a
        // previous sample downward. Chunk usage and the final message are one call.
        if let Some(usage) = final_usage.or(usage) {
            message.tokens = Some(usage.tokens()?);
        }
        if let Some(token_time) = first_token {
            if let Some(turn) = turns.get_mut(&turn) {
                if token_time >= turn.started && turn.first_token.is_none() {
                    turn.first_token = Some((token_time, provider.into(), model.into()));
                }
            }
        }
        if row.kind == "tool/call" {
            if let Some(name) = data.name {
                message
                    .tools
                    .insert(data.call_id.unwrap_or_else(|| format!("call:{seq}")), name);
            }
        }
        if row.kind == "step/end" {
            message.interval = (time > attempt.started).then_some((attempt.started, time));
        }
    }
    if header.is_seeded == Some(true) && header.seed_length.is_none() && !saw_inherited_cut {
        return Err(corrupt());
    }
    let mut outcome = snapshot.emit(change, sink, cancel)?;
    for (id, turn) in turns {
        check_cancel(cancel)?;
        if let Some((time, provider, model)) = turn.first_token {
            sink(ParsedRecord::FirstTokenLatency(FirstTokenLatency {
                byte_offset: id,
                turn_id: Some(format!("{}:{id}", header.id)),
                occurred_at_ms: time,
                model_provider: provider,
                model_raw: model,
                latency_ms: time - turn.started,
            }))?;
            outcome.records_emitted += 1;
        }
    }
    Ok(outcome)
}
