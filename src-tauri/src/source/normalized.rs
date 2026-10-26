//! Shared boundary for local agents. Only structured usage and tool names cross it.
use std::collections::{BTreeMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};

use super::{
    ActivitySegment, ModelChanged, ParsedRecord, SessionMetadata, SourceChange, SourceCursor,
    StreamOutcome, ToolEvent, UsageEvent,
};
use crate::activity::{ToolInvocation, classify};
use crate::error::{AppError, AppResult};
use crate::pricing::{canonical_model_name, normalize_model_id};

#[derive(Debug, Default, Clone)]
pub(super) struct Tokens {
    pub fresh: u64,
    pub cached: u64,
    pub write: u64,
    pub write_1h: u64,
    /// Includes reasoning, matching Chronolume's existing output contract.
    pub output: u64,
    pub reasoning: u64,
}

#[derive(Debug, Clone)]
pub(super) struct Message {
    pub id: String,
    pub timestamp: i64,
    pub provider: String,
    pub model: String,
    pub tokens: Option<Tokens>,
    pub reported_cost: Option<i64>,
    pub tools: BTreeMap<String, String>,
    pub interval: Option<(i64, i64)>,
}

impl Message {
    /// Streaming transcripts can repeat a message with increasingly complete usage.
    pub fn merge(&mut self, next: Self) {
        self.timestamp = self.timestamp.min(next.timestamp);
        if self.model == "unknown" && next.model != "unknown" {
            self.model = next.model;
        }
        if self.provider == "unknown" && next.provider != "unknown" {
            self.provider = next.provider;
        }
        self.tools.extend(next.tools);
        if next.interval.is_some() {
            self.interval = next.interval;
        }
        if next.reported_cost.is_some() {
            self.reported_cost = next.reported_cost;
        }
        if let Some(next) = next.tokens {
            if let Some(tokens) = &mut self.tokens {
                tokens.fresh = tokens.fresh.max(next.fresh);
                tokens.cached = tokens.cached.max(next.cached);
                tokens.write = tokens.write.max(next.write);
                tokens.write_1h = tokens.write_1h.max(next.write_1h).min(tokens.write);
                tokens.output = tokens.output.max(next.output);
                tokens.reasoning = tokens.reasoning.max(next.reasoning);
            } else {
                self.tokens = Some(next);
            }
        }
    }
}

#[derive(Default)]
pub(super) struct Snapshot {
    pub cwd: Option<String>,
    pub parent: Option<String>,
    pub archived: bool,
    pub created: Option<i64>,
    pub messages: BTreeMap<String, Message>,
    pub parse_failures: u64,
    pub incomplete_tail: bool,
    pub bytes_read: u64,
}

impl Snapshot {
    pub fn insert(&mut self, message: Message) {
        if let Some(existing) = self.messages.get_mut(&message.id) {
            existing.merge(message);
        } else {
            self.messages.insert(message.id.clone(), message);
        }
    }

    pub fn emit(
        self,
        change: &SourceChange,
        sink: &mut dyn FnMut(ParsedRecord) -> AppResult<()>,
        cancel: &AtomicBool,
    ) -> AppResult<StreamOutcome> {
        check_cancel(cancel)?;
        let session_id = change
            .session_id
            .clone()
            .unwrap_or_else(|| change.source_key.clone());
        let agent = session_id
            .split(':')
            .next()
            .unwrap_or("unknown")
            .to_string();
        let mut messages: Vec<_> = self.messages.into_values().collect();
        messages.sort_by(|a, b| (a.timestamp, &a.id).cmp(&(b.timestamp, &b.id)));
        let mut records = 0;
        let mut emit = |record| {
            sink(record)?;
            records += 1;
            Ok::<_, AppError>(())
        };
        emit(ParsedRecord::SessionMetadata(SessionMetadata {
            event_ordinal: 0,
            occurred_at_ms: self
                .created
                .or_else(|| messages.first().map(|m| m.timestamp)),
            session_id,
            cwd: self.cwd,
            model_provider: None,
            legacy_model: None,
        }))?;
        emit(ParsedRecord::SessionAttributes {
            parent_session_id: self.parent,
            archived: self.archived,
        })?;
        let mut seen_tools = HashSet::new();
        let mut interval_end = i64::MIN;
        for (index, mut message) in messages.into_iter().enumerate() {
            message.model = canonical_model_name(&message.model);
            check_cancel(cancel)?;
            let ordinal = index as u64 + 1;
            let pricing_id = normalize_model_id(&message.model).exact;
            emit(ParsedRecord::ModelChanged(ModelChanged {
                byte_offset: ordinal,
                event_ordinal: ordinal,
                occurred_at_ms: Some(message.timestamp),
                model_provider: message.provider.clone(),
                model_raw: message.model.clone(),
                pricing_model_id: pricing_id.clone(),
            }))?;
            if let Some(tokens) = message.tokens {
                let input = tokens
                    .fresh
                    .saturating_add(tokens.cached)
                    .saturating_add(tokens.write);
                emit(ParsedRecord::Usage(UsageEvent {
                    byte_offset: ordinal,
                    event_ordinal: ordinal,
                    occurred_at_ms: message.timestamp,
                    model_provider: message.provider,
                    model_raw: message.model,
                    pricing_model_id: pricing_id,
                    input_tokens: input,
                    fresh_input_tokens: tokens.fresh,
                    cached_input_tokens: tokens.cached,
                    cache_write_tokens: tokens.write,
                    cache_write_1h_tokens: tokens.write_1h,
                    output_tokens: tokens.output,
                    reasoning_tokens: tokens.reasoning,
                    total_tokens: input.saturating_add(tokens.output),
                    reported_cost_microusd: message.reported_cost,
                    native_event_id: Some(format!("{agent}:message:{}", message.id)),
                }))?;
            }
            if let Some((start, end)) = message.interval {
                let start = start.max(interval_end);
                if end > start {
                    emit(ParsedRecord::Activity(ActivitySegment {
                        byte_offset: ordinal,
                        event_ordinal: ordinal,
                        started_at_ms: start,
                        ended_at_ms: end,
                        active_ms: (end - start) as u64,
                    }))?;
                    interval_end = end;
                }
            }
            for (sub_index, (id, name)) in message.tools.into_iter().enumerate() {
                if !seen_tools.insert(id) {
                    continue;
                }
                for tool in classify(ToolInvocation {
                    tool_name: &name,
                    raw_arguments: None,
                }) {
                    emit(ParsedRecord::Tool(ToolEvent {
                        byte_offset: ordinal,
                        event_ordinal: ordinal,
                        sub_index: sub_index as u32,
                        occurred_at_ms: message.timestamp,
                        tool_name: tool.tool_name,
                        category: tool.category,
                        operation_kind: tool.operation_kind,
                    }))?;
                }
            }
        }
        Ok(StreamOutcome {
            cursor: SourceCursor {
                safe_offset: change.file_size,
                complete_line_offset: change.file_size,
                ..SourceCursor::default()
            },
            bytes_read: self.bytes_read,
            records_emitted: records,
            parse_failures: self.parse_failures,
            incomplete_tail: self.incomplete_tail,
            cancelled: false,
        })
    }
}

pub(super) fn check_cancel(cancel: &AtomicBool) -> AppResult<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(AppError::new("sync_cancelled", "同步已取消"))
    } else {
        Ok(())
    }
}

pub(super) fn number(value: Option<&serde_json::Value>) -> u64 {
    value
        .and_then(serde_json::Value::as_u64)
        .filter(|n| *n <= i64::MAX as u64)
        .unwrap_or(0)
}

pub(super) fn timestamp(value: Option<&serde_json::Value>) -> Option<i64> {
    let value = value?;
    if let Some(ms) = value.as_i64() {
        return (ms >= 0).then_some(ms);
    }
    chrono::DateTime::parse_from_rfc3339(value.as_str()?)
        .ok()
        .map(|time| time.timestamp_millis())
}

pub(super) fn cost(value: Option<&serde_json::Value>) -> Option<i64> {
    let cost = value?.as_f64()? * 1_000_000.0;
    (cost.is_finite() && cost >= 0.0 && cost < i64::MAX as f64).then(|| cost.round() as i64)
}
