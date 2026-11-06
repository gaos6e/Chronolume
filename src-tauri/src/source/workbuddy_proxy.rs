//! Read the completion records emitted by the optional local WorkBuddyProxy.
//! Its headless CLI disables transcripts, so the proxy supplies request IDs,
//! timestamps, and native aggregate input/output counts without session IDs.
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::atomic::AtomicBool;

use serde::Deserialize;

use super::normalized::{Message, Snapshot, Tokens, check_cancel};
use super::{
    ChangeAction, ParsedRecord, SourceChange, SourceCursor, SourceKind, StreamOutcome,
    WORKBUDDY_PROXY_SESSION_PREFIX, modified_ns, prefix_hash,
};
use crate::error::AppResult;

fn log_id(name: &str) -> Option<&str> {
    let id = name.strip_prefix("proxy-")?.strip_suffix(".stdout.log")?;
    let (date, time) = id.split_once('-')?;
    (date.len() == 8
        && time.len() == 6
        && date.bytes().chain(time.bytes()).all(|b| b.is_ascii_digit()))
    .then_some(id)
}

pub(super) fn has_logs(root: &Path) -> bool {
    fs::read_dir(root).is_ok_and(|mut entries| {
        entries.any(|entry| {
            entry.is_ok_and(|entry| {
                log_id(&entry.file_name().to_string_lossy()).is_some()
                    && entry.file_type().is_ok_and(|kind| kind.is_file())
            })
        })
    })
}

pub(super) fn discover(root: &Path) -> AppResult<Vec<SourceChange>> {
    if !root.try_exists()? {
        return Ok(Vec::new());
    }
    let mut sources = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        let Some(id) = log_id(&name) else {
            continue;
        };
        if !entry.file_type()?.is_file() {
            continue;
        }
        let path = entry.path();
        let metadata = entry.metadata()?;
        if metadata.len() == 0 {
            continue;
        }
        let session_id = format!("{WORKBUDDY_PROXY_SESSION_PREFIX}{id}");
        sources.push(SourceChange {
            source_key: session_id.clone(),
            session_id: Some(session_id),
            relative_path: format!("proxy/{name}"),
            prefix_hash: prefix_hash(&path)?,
            path,
            kind: SourceKind::Session,
            action: ChangeAction::Replay,
            file_size: metadata.len(),
            mtime_ns: modified_ns(&metadata),
            cursor: SourceCursor::default(),
        });
    }
    sources.sort_by(|a, b| a.source_key.cmp(&b.source_key));
    Ok(sources)
}

#[derive(Deserialize)]
struct LogRow {
    event: Option<String>,
    time: Option<String>,
    request_id: Option<String>,
    model: Option<String>,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
}

impl LogRow {
    fn completion(self) -> Option<Message> {
        if self.event.as_deref() != Some("completion") {
            return None;
        }
        let request = self.request_id.filter(|id| !id.is_empty())?;
        let time = chrono::DateTime::parse_from_rfc3339(self.time.as_deref()?).ok()?;
        let (input, output) = (self.input_tokens?, self.output_tokens?);
        let total = input.checked_add(output)?;
        if total == 0 || total > i64::MAX as u64 {
            return None;
        }
        Some(Message {
            id: format!("proxy:{request}"),
            timestamp: time.timestamp_millis(),
            provider: "workbuddy".into(),
            model: self
                .model
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| "unknown".into()),
            tokens: Some(Tokens {
                fresh: input,
                output,
                ..Tokens::default()
            }),
            reported_cost: None,
            tools: Default::default(),
            interval: None,
        })
    }
}

pub(super) fn stream(
    change: &SourceChange,
    sink: &mut dyn FnMut(ParsedRecord) -> AppResult<()>,
    cancel: &AtomicBool,
) -> AppResult<StreamOutcome> {
    let mut snapshot = Snapshot {
        // The proxy directory is the only known workspace. A rotated log is one
        // import batch because the bridge has no conversation identity.
        cwd: change
            .path
            .parent()
            .and_then(Path::parent)
            .map(|path| path.to_string_lossy().into_owned()),
        ..Snapshot::default()
    };
    let mut reader = BufReader::new(File::open(&change.path)?);
    let mut line = Vec::new();
    loop {
        check_cancel(cancel)?;
        line.clear();
        let bytes = reader.read_until(b'\n', &mut line)?;
        if bytes == 0 {
            break;
        }
        snapshot.bytes_read += bytes as u64;
        // The stdout file may also contain non-JSON server diagnostics.
        if line.iter().find(|b| !b.is_ascii_whitespace()) != Some(&b'{') {
            continue;
        }
        let row: LogRow = match serde_json::from_slice(&line) {
            Ok(row) => row,
            Err(_) if line.last() != Some(&b'\n') => {
                snapshot.incomplete_tail = true;
                break;
            }
            Err(_) => {
                snapshot.parse_failures += 1;
                continue;
            }
        };
        if let Some(message) = row.completion() {
            snapshot
                .messages
                .entry(message.id.clone())
                .or_insert(message);
        }
    }
    if snapshot.messages.is_empty() {
        return Ok(StreamOutcome {
            cursor: SourceCursor {
                safe_offset: change.file_size,
                complete_line_offset: change.file_size,
                ..SourceCursor::default()
            },
            bytes_read: snapshot.bytes_read,
            records_emitted: 0,
            parse_failures: snapshot.parse_failures,
            incomplete_tail: snapshot.incomplete_tail,
            cancelled: false,
        });
    }
    snapshot.emit(change, sink, cancel)
}
