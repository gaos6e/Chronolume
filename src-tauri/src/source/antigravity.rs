use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use rusqlite::{Connection, OpenFlags};
use serde::Deserialize;

use super::normalized::{Message, Snapshot, Tokens, check_cancel};
use super::{
    ChangeAction, ParsedRecord, SourceChange, SourceCursor, SourceKind, StreamOutcome, modified_ns,
    prefix_hash,
};
use crate::error::{AppError, AppResult};

fn transcript(conversation: &Path) -> AppResult<Option<PathBuf>> {
    let logs = conversation.join(".system_generated/logs");
    for name in ["transcript.jsonl", "transcript_full.jsonl"] {
        let path = logs.join(name);
        match fs::metadata(&path) {
            Ok(metadata) if metadata.is_file() => return Ok(Some(path)),
            Ok(_) => continue,
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(None)
}

pub(super) fn has_transcripts(root: &Path) -> bool {
    fs::read_dir(root.join("brain")).is_ok_and(|entries| {
        entries.filter_map(Result::ok).any(|e| {
            e.file_type().is_ok_and(|t| t.is_dir())
                && transcript(&e.path()).ok().flatten().is_some()
        })
    })
}

#[derive(Default)]
struct Summary {
    cwd: Option<String>,
    parent: Option<String>,
}

fn workspace(raw: &str) -> Option<String> {
    let raw = raw.trim();
    let uris: Vec<String>;
    let uri = if raw.starts_with('[') {
        uris = serde_json::from_str(raw).ok()?;
        let [single] = uris.as_slice() else {
            return None;
        };
        single.as_str()
    } else {
        raw
    };
    if !uri.starts_with("file:") || uri.matches("file:").count() != 1 {
        return None;
    }
    reqwest::Url::parse(uri)
        .ok()?
        .to_file_path()
        .ok()?
        .to_str()
        .map(str::to_owned)
}

// Only project indexing metadata. Titles, previews, raw summaries and the FTS tables
// contain conversation text and are not needed for attribution.
fn summaries(root: &Path, session: Option<&str>) -> AppResult<BTreeMap<String, Summary>> {
    let path = root.join("conversation_summaries.db");
    if !path.try_exists()? {
        return Ok(BTreeMap::new());
    }
    let mut uri = reqwest::Url::from_file_path(&path).map_err(|_| AppError::filesystem())?;
    // SQLite can create WAL/SHM files even on a read-only database handle. Read
    // checkpointed metadata without sidecars, or join an existing live WAL read-only.
    let live_wal = path.with_extension("db-shm").is_file()
        && path
            .with_extension("db-wal")
            .metadata()
            .is_ok_and(|meta| meta.len() > 0);
    uri.set_query(Some(if live_wal {
        "mode=ro&readonly_shm=1"
    } else {
        "mode=ro&immutable=1"
    }));
    let db = Connection::open_with_flags(
        uri.as_str(),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )?;
    let mut columns = db.prepare("SELECT name FROM pragma_table_info('conversation_summaries')")?;
    let columns = columns
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<BTreeSet<_>, _>>()?;
    if !columns.contains("conversation_id") {
        return Ok(BTreeMap::new());
    }
    let field = |name| if columns.contains(name) { name } else { "NULL" };
    let mut sql = format!(
        "SELECT conversation_id, {}, {} FROM conversation_summaries",
        field("workspace_uris"),
        field("parent_conversation_id")
    );
    if session.is_some() {
        sql.push_str(" WHERE conversation_id = ?1");
    }
    let mut query = db.prepare(&sql)?;
    let rows = query.query_map(rusqlite::params_from_iter(session), |row| {
        let uris: Option<String> = row.get(1)?;
        let parent: Option<String> = row.get(2)?;
        Ok((
            row.get(0)?,
            Summary {
                cwd: uris.as_deref().and_then(workspace),
                parent: parent
                    .filter(|id| !id.is_empty())
                    .map(|id| format!("antigravity:{id}")),
            },
        ))
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

#[derive(Default)]
struct Conversation {
    files: BTreeSet<(u128, PathBuf)>,
    summary: Summary,
}

impl Conversation {
    fn add(&mut self, path: PathBuf, summary: Option<Summary>) -> AppResult<()> {
        let stamp = modified_ns(&fs::metadata(&path)?);
        self.files.insert((stamp, path));
        if let Some(meta) = summary {
            self.summary.cwd = self.summary.cwd.take().or(meta.cwd);
            self.summary.parent = self.summary.parent.take().or(meta.parent);
        }
        Ok(())
    }

    fn change(&self, id: &str) -> AppResult<SourceChange> {
        let mut bytes = 0;
        let mut hash = blake3::Hasher::new();
        for (stamp, path) in &self.files {
            let size = fs::metadata(path)?.len();
            bytes += size;
            hash.update(path.as_os_str().as_encoded_bytes());
            hash.update(&size.to_le_bytes());
            hash.update(&stamp.to_le_bytes());
            hash.update(prefix_hash(path)?.as_bytes());
        }
        for value in [&self.summary.cwd, &self.summary.parent] {
            hash.update(value.as_deref().unwrap_or_default().as_bytes());
            hash.update(&[0]);
        }
        let (stamp, path) = self.files.last().ok_or_else(AppError::filesystem)?;
        let key = format!("antigravity:{id}");
        Ok(SourceChange {
            session_id: Some(key.clone()),
            source_key: key,
            path: path.clone(),
            relative_path: format!("brain/{id}"),
            kind: SourceKind::Session,
            action: ChangeAction::Replay,
            file_size: bytes,
            mtime_ns: *stamp,
            prefix_hash: hash.finalize().to_hex().to_string(),
            cursor: SourceCursor::default(),
        })
    }
}

pub(super) fn discover(roots: &[PathBuf]) -> AppResult<Vec<SourceChange>> {
    let mut sessions = BTreeMap::<String, Conversation>::new();
    for root in roots {
        let entries = match fs::read_dir(root.join("brain")) {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        let mut metadata = summaries(root, None)?;
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            if let Some(path) = transcript(&entry.path())? {
                let id = entry
                    .file_name()
                    .into_string()
                    .map_err(|_| AppError::filesystem())?;
                let summary = metadata.remove(&id);
                sessions.entry(id).or_default().add(path, summary)?;
            }
        }
    }
    sessions
        .into_iter()
        .map(|(id, session)| session.change(&id))
        .collect()
}

#[derive(Deserialize)]
struct Step {
    step_index: u64,
    #[serde(rename = "type")]
    kind: String,
    created_at: chrono::DateTime<chrono::FixedOffset>,
    model: Option<String>,
    input_tokens: Option<u64>,
    cache_read_tokens: Option<u64>,
    output_tokens: Option<u64>,
    tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Deserialize)]
struct ToolCall {
    name: String,
}

fn read_steps(
    path: &Path,
    id: &str,
    snapshot: &mut Snapshot,
    cancel: &AtomicBool,
) -> AppResult<()> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut line = Vec::new();
    loop {
        check_cancel(cancel)?;
        line.clear();
        let n = reader.read_until(b'\n', &mut line)?;
        if n == 0 {
            break;
        }
        snapshot.bytes_read += n as u64;
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let step = match serde_json::from_slice::<Step>(&line) {
            Ok(step) => step,
            Err(error) if error.is_eof() && !line.ends_with(b"\n") => {
                snapshot.incomplete_tail = true;
                break;
            }
            Err(_) => {
                return Err(AppError::new(
                    "invalid_antigravity_step",
                    "Antigravity 会话步骤格式无效",
                ));
            }
        };
        let time = step.created_at.timestamp_millis();
        snapshot.created = Some(snapshot.created.map_or(time, |old| old.min(time)));
        if step.kind != "PLANNER_RESPONSE" {
            continue;
        }
        let key = format!("{id}:{}", step.step_index);
        let tokens = (step.input_tokens.is_some()
            || step.cache_read_tokens.is_some()
            || step.output_tokens.is_some())
        .then(|| Tokens {
            fresh: step.input_tokens.unwrap_or(0),
            cached: step.cache_read_tokens.unwrap_or(0),
            output: step.output_tokens.unwrap_or(0),
            ..Default::default()
        });
        let tools = step
            .tool_calls
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .map(|(i, tool)| (format!("{key}:{i}"), tool.name))
            .collect();
        // A step is a snapshot, including downward corrections. Newer copies replay last.
        snapshot.messages.insert(
            key.clone(),
            Message {
                id: key,
                timestamp: time,
                provider: "unknown".into(),
                model: step.model.unwrap_or_else(|| "unknown".into()),
                tokens,
                tools,
                reported_cost: None,
                interval: None,
            },
        );
    }
    Ok(())
}

pub(super) fn stream(
    change: &SourceChange,
    roots: &[PathBuf],
    sink: &mut dyn FnMut(ParsedRecord) -> AppResult<()>,
    cancel: &AtomicBool,
) -> AppResult<StreamOutcome> {
    let id = change
        .source_key
        .strip_prefix("antigravity:")
        .ok_or_else(AppError::filesystem)?;
    let mut session = Conversation::default();
    for root in roots {
        if let Some(path) = transcript(&root.join("brain").join(id))? {
            session.add(path, summaries(root, Some(id))?.remove(id))?;
        }
    }
    // A concurrent deletion or directory switch must not replace history with an empty snapshot.
    if session.change(id)?.prefix_hash != change.prefix_hash {
        return Err(AppError::new(
            "antigravity_source_changed",
            "Antigravity 会话已更改，请重新同步",
        ));
    }
    let mut snapshot = Snapshot {
        cwd: session.summary.cwd,
        parent: session.summary.parent,
        ..Default::default()
    };
    for (_, path) in session.files {
        read_steps(&path, id, &mut snapshot, cancel)?;
    }
    snapshot.emit(change, sink, cancel)
}
