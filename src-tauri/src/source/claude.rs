use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::sync::atomic::AtomicBool;

use serde::Deserialize;
use serde_json::Value;

use super::normalized::{Message, Snapshot, Tokens, check_cancel, cost, number, timestamp};
use super::{ParsedRecord, SourceChange, StreamOutcome};
use crate::error::AppResult;

// Deserialize only the fields needed by analytics. Content text and tool inputs are skipped.
#[derive(Deserialize)]
struct Entry {
    #[serde(rename = "type")]
    kind: Option<String>,
    timestamp: Option<Value>,
    cwd: Option<String>,
    message: Option<Assistant>,
    #[serde(rename = "requestId")]
    request_id: Option<String>,
    #[serde(rename = "parentSessionId", alias = "forkedFromSessionId")]
    parent_session_id: Option<String>,
    #[serde(rename = "costUSD")]
    cost: Option<Value>,
}
#[derive(Deserialize)]
struct Assistant {
    id: Option<String>,
    model: Option<String>,
    usage: Option<Value>,
    #[serde(default, deserialize_with = "blocks")]
    content: Vec<Block>,
}
#[derive(Deserialize)]
struct Block {
    #[serde(rename = "type")]
    kind: Option<String>,
    id: Option<String>,
    name: Option<String>,
}

fn blocks<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Vec<Block>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Content {
        Blocks(Vec<Block>),
        Text(serde::de::IgnoredAny),
    }
    Ok(match Content::deserialize(deserializer)? {
        Content::Blocks(blocks) => blocks,
        Content::Text(_) => Vec::new(),
    })
}

pub(super) fn stream(
    change: &SourceChange,
    sink: &mut dyn FnMut(ParsedRecord) -> AppResult<()>,
    cancel: &AtomicBool,
) -> AppResult<StreamOutcome> {
    let mut snapshot = Snapshot::default();
    // Parent identity comes from discovery rather than the subagent's repeated sessionId.
    if let Some(id) = change
        .session_id
        .as_deref()
        .and_then(|id| id.split_once(":agent:").map(|pair| pair.0))
    {
        snapshot.parent = Some(id.to_string());
    }
    let mut reader = BufReader::new(File::open(&change.path)?);
    let mut line = Vec::new();
    let mut offset = 0_u64;
    loop {
        check_cancel(cancel)?;
        line.clear();
        let bytes = reader.read_until(b'\n', &mut line)?;
        if bytes == 0 {
            break;
        }
        snapshot.bytes_read += bytes as u64;
        let complete = line.last() == Some(&b'\n');
        let entry: Entry = match serde_json::from_slice(&line) {
            Ok(entry) => entry,
            Err(_) => {
                if complete {
                    snapshot.parse_failures += 1;
                } else {
                    snapshot.incomplete_tail = true;
                }
                offset += bytes as u64;
                continue;
            }
        };
        if snapshot.cwd.is_none() {
            snapshot.cwd = entry.cwd;
        }
        if snapshot.parent.is_none() {
            snapshot.parent = entry.parent_session_id.map(|id| format!("claude:{id}"));
        }
        let time = timestamp(entry.timestamp.as_ref());
        snapshot.created = match (snapshot.created, time) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        if entry.kind.as_deref() != Some("assistant") {
            offset += bytes as u64;
            continue;
        }
        let (Some(message), Some(time)) = (entry.message, time) else {
            snapshot.parse_failures += 1;
            offset += bytes as u64;
            continue;
        };
        let id = message
            .id
            .map(|id| format!("{id}:{}", entry.request_id.unwrap_or_default()))
            .unwrap_or_else(|| format!("{}:{offset}", change.source_key));
        let mut tools = BTreeMap::new();
        for (index, block) in message.content.into_iter().enumerate() {
            if block.kind.as_deref() == Some("tool_use") {
                if let Some(name) = block.name {
                    tools.insert(block.id.unwrap_or_else(|| format!("{id}:{index}")), name);
                }
            }
        }
        let tokens = message.usage.map(|usage| {
            let write_1h = number(usage.pointer("/cache_creation/ephemeral_1h_input_tokens"));
            let write_5m = number(usage.pointer("/cache_creation/ephemeral_5m_input_tokens"));
            let write = number(usage.get("cache_creation_input_tokens"))
                .max(write_1h.saturating_add(write_5m));
            let output = number(usage.get("output_tokens"));
            Tokens {
                fresh: number(usage.get("input_tokens")),
                cached: number(usage.get("cache_read_input_tokens")),
                write,
                write_1h,
                output,
                reasoning: number(usage.get("reasoning_tokens")).min(output),
            }
        });
        snapshot.insert(Message {
            id,
            timestamp: time,
            provider: "anthropic".into(),
            model: message.model.unwrap_or_else(|| "unknown".into()),
            tokens,
            tools,
            reported_cost: cost(entry.cost.as_ref()),
            interval: None,
        });
        offset += bytes as u64;
    }
    snapshot.emit(change, sink, cancel)
}
