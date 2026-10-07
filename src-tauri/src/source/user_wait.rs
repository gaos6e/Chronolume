use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeInterval {
    pub start: i64,
    pub end: i64,
}

impl TimeInterval {
    pub fn duration_ms(self) -> u64 {
        self.end.saturating_sub(self.start).max(0) as u64
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum CallKind {
    Question,
    AsyncQuestion,
    UserPause,
    Work,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingCall {
    start: i64,
    kind: CallKind,
}

/// Checkpoints contain only timing and opaque call IDs, never question or answer text.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UserWaitTiming {
    pending: HashMap<String, PendingCall>,
    async_question_pending: bool,
    non_work_ids: Vec<String>,
    waits: Vec<TimeInterval>,
    work: Vec<TimeInterval>,
    completed_waits: Vec<TimeInterval>,
    last_timestamp: Option<i64>,
}

impl UserWaitTiming {
    pub fn observe(
        &mut self,
        event_type: &str,
        payload: &serde_json::Map<String, Value>,
        time: i64,
    ) {
        self.last_timestamp = Some(self.last_timestamp.map_or(time, |last| last.max(time)));
        match (event_type, payload.get("type").and_then(Value::as_str)) {
            ("response_item", Some("function_call" | "custom_tool_call")) => {
                let Some(id) = payload.get("call_id").and_then(Value::as_str) else {
                    return;
                };
                let name = payload.get("name").and_then(Value::as_str).unwrap_or("");
                let short_name = name.rsplit('.').next().unwrap_or(name);
                let clock_sleep = name == "clock.sleep"
                    || (name == "sleep"
                        && payload.get("namespace").and_then(Value::as_str) == Some("clock"));
                let kind = match short_name {
                    "request_user_input" => CallKind::Question,
                    "request_user_input_async" => {
                        self.async_question_pending = true;
                        CallKind::AsyncQuestion
                    }
                    _ if clock_sleep && self.async_question_pending => CallKind::UserPause,
                    _ => CallKind::Work,
                };
                if !matches!(kind, CallKind::Work) {
                    self.non_work_ids.push(id.to_owned());
                }
                self.pending
                    .insert(id.to_owned(), PendingCall { start: time, kind });
            }
            ("response_item", Some("function_call_output" | "custom_tool_call_output")) => {
                if let Some(call) = payload
                    .get("call_id")
                    .and_then(Value::as_str)
                    .and_then(|id| self.pending.remove(id))
                {
                    self.close_call(call, time);
                }
            }
            ("response_item", Some("message"))
                if payload.get("role").and_then(Value::as_str) == Some("user") =>
            {
                self.user_answer(time)
            }
            ("event_msg", Some("user_message")) => self.user_answer(time),
            ("event_msg", Some("item_completed")) => {
                let Some(item) = payload.get("item") else {
                    return;
                };
                let kind = item.get("type").and_then(Value::as_str).unwrap_or("");
                if kind == "UserMessage" || kind == "userMessage" {
                    self.user_answer(time);
                    return;
                }
                let id = item.get("id").and_then(Value::as_str).unwrap_or("");
                let extension = item.get("kind").and_then(Value::as_str).unwrap_or("");
                if self.non_work_ids.iter().any(|non_work| non_work == id)
                    || matches!(
                        extension,
                        "clock.sleep"
                            | "functions.request_user_input"
                            | "functions.request_user_input_async"
                    )
                {
                    return;
                }
                if let (Some(start), Some(end)) = (
                    payload.get("started_at_ms").and_then(Value::as_i64),
                    payload.get("completed_at_ms").and_then(Value::as_i64),
                ) {
                    if end > start {
                        self.work.push(TimeInterval { start, end });
                    }
                }
            }
            _ => {}
        }
    }

    fn close_call(&mut self, call: PendingCall, end: i64) {
        if end <= call.start {
            return;
        }
        let interval = TimeInterval {
            start: call.start,
            end,
        };
        match call.kind {
            CallKind::Question | CallKind::UserPause => self.waits.push(interval),
            CallKind::Work => self.work.push(interval),
            CallKind::AsyncQuestion => {}
        }
    }

    fn user_answer(&mut self, time: i64) {
        self.async_question_pending = false;
        let pauses: Vec<_> = self
            .pending
            .iter()
            .filter_map(|(id, call)| {
                matches!(call.kind, CallKind::UserPause | CallKind::Question).then_some(id.clone())
            })
            .collect();
        for id in pauses {
            if let Some(call) = self.pending.remove(&id) {
                self.close_call(call, time);
            }
        }
    }

    pub fn excluded_intervals(&self) -> Vec<TimeInterval> {
        let mut all = self.completed_waits.clone();
        if let Some(end) = self.last_timestamp {
            all.extend(self.current_waits(end));
        }
        merge_intervals(all)
    }

    fn current_waits(&self, end: i64) -> Vec<TimeInterval> {
        let mut waits = self.waits.clone();
        let mut work = self.work.clone();
        for call in self.pending.values() {
            let interval = TimeInterval {
                start: call.start,
                end,
            };
            match call.kind {
                CallKind::Question | CallKind::UserPause => waits.push(interval),
                CallKind::Work => work.push(interval),
                CallKind::AsyncQuestion => {}
            }
        }
        subtract_intervals(&merge_intervals(waits), &merge_intervals(work))
    }

    pub fn start_task(&mut self, time: i64) {
        self.seal(time);
    }

    pub fn seal(&mut self, end: i64) {
        let waits = self.current_waits(end);
        let mut completed_waits = std::mem::take(&mut self.completed_waits);
        completed_waits.extend(waits);
        *self = Self {
            completed_waits: merge_intervals(completed_waits),
            ..Self::default()
        };
    }

    pub fn active_pieces(
        &mut self,
        start: i64,
        end: i64,
        duration_ms: u64,
    ) -> Vec<(TimeInterval, u64)> {
        let task = TimeInterval { start, end };
        if start == end {
            // Rounded timestamps may coincide while the reported duration is nonzero.
            self.seal(end);
            return vec![(task, duration_ms)];
        }
        let waits: Vec<_> = self
            .current_waits(end)
            .into_iter()
            .filter_map(|wait| {
                let clipped = TimeInterval {
                    start: wait.start.max(start),
                    end: wait.end.min(end),
                };
                (clipped.end > clipped.start).then_some(clipped)
            })
            .collect();
        let excluded_ms = waits.iter().map(|wait| wait.duration_ms()).sum::<u64>();
        let active_ms = duration_ms.saturating_sub(excluded_ms);
        let pieces = subtract_intervals(&[task], &waits);
        self.seal(end);
        if pieces.is_empty() {
            return vec![(task, 0)];
        }
        let span: u64 = pieces.iter().map(|piece| piece.duration_ms()).sum();
        if span == 0 {
            return vec![(task, active_ms)];
        }
        let mut allocated = 0;
        let count = pieces.len();
        pieces
            .into_iter()
            .enumerate()
            .map(|(index, piece)| {
                let duration = if index + 1 == count {
                    active_ms - allocated
                } else {
                    (u128::from(active_ms) * u128::from(piece.duration_ms()) / u128::from(span))
                        as u64
                };
                allocated += duration;
                (piece, duration)
            })
            .collect()
    }
}

pub fn merge_intervals(mut intervals: Vec<TimeInterval>) -> Vec<TimeInterval> {
    intervals.retain(|interval| interval.end > interval.start);
    intervals.sort_unstable_by_key(|interval| interval.start);
    let mut merged: Vec<TimeInterval> = Vec::new();
    for interval in intervals {
        if let Some(previous) = merged.last_mut() {
            if interval.start <= previous.end {
                previous.end = previous.end.max(interval.end);
                continue;
            }
        }
        merged.push(interval);
    }
    merged
}

pub fn subtract_intervals(
    intervals: &[TimeInterval],
    excluded: &[TimeInterval],
) -> Vec<TimeInterval> {
    let mut result = Vec::new();
    for interval in intervals {
        let mut start = interval.start;
        for gap in excluded {
            if gap.end <= start {
                continue;
            }
            if gap.start >= interval.end {
                break;
            }
            if gap.start > start {
                result.push(TimeInterval {
                    start,
                    end: gap.start,
                });
            }
            start = start.max(gap.end);
            if start >= interval.end {
                break;
            }
        }
        if start < interval.end {
            result.push(TimeInterval {
                start,
                end: interval.end,
            });
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn record(timing: &mut UserWaitTiming, time: i64, event: &str, payload: Value) {
        timing.observe(event, payload.as_object().unwrap(), time);
    }

    fn call(timing: &mut UserWaitTiming, time: i64, name: &str, id: &str) {
        record(
            timing,
            time,
            "response_item",
            json!({"type":"function_call","name":name,"call_id":id}),
        );
    }

    fn output(timing: &mut UserWaitTiming, time: i64, id: &str) {
        record(
            timing,
            time,
            "response_item",
            json!({"type":"function_call_output","call_id":id}),
        );
    }

    #[test]
    fn plan_question_excludes_only_answer_wait_and_keeps_planning_work() {
        let mut timing = UserWaitTiming::default();
        call(
            &mut timing,
            20_000,
            "functions.request_user_input",
            "question",
        );
        output(&mut timing, 100_000, "question");
        let pieces = timing.active_pieces(0, 120_000, 120_000);
        assert_eq!(
            pieces,
            vec![
                (
                    TimeInterval {
                        start: 0,
                        end: 20_000
                    },
                    20_000
                ),
                (
                    TimeInterval {
                        start: 100_000,
                        end: 120_000
                    },
                    20_000
                )
            ]
        );
    }

    #[test]
    fn async_question_keeps_followup_work_and_excludes_only_explicit_pause() {
        let mut timing = UserWaitTiming::default();
        call(&mut timing, 1000, "request_user_input_async", "question");
        output(&mut timing, 1100, "question");
        call(&mut timing, 2000, "exec", "work");
        output(&mut timing, 9000, "work");
        record(
            &mut timing,
            10_000,
            "response_item",
            json!({"type":"function_call","name":"sleep","namespace":"clock","call_id":"pause"}),
        );
        record(
            &mut timing,
            25_000,
            "response_item",
            json!({"type":"message","role":"user","content":"PRIVATE_ANSWER"}),
        );
        output(&mut timing, 30_000, "pause");
        assert_eq!(
            timing
                .active_pieces(0, 40_000, 40_000)
                .iter()
                .map(|(_, ms)| ms)
                .sum::<u64>(),
            25_000
        );
        assert!(
            !serde_json::to_string(&timing)
                .unwrap()
                .contains("PRIVATE_ANSWER")
        );
    }

    #[test]
    fn question_without_pause_and_sleep_without_question_keep_their_time() {
        for ask in [true, false] {
            let mut timing = UserWaitTiming::default();
            if ask {
                call(&mut timing, 1000, "request_user_input_async", "question");
                output(&mut timing, 1100, "question");
            } else {
                call(&mut timing, 10_000, "clock.sleep", "pause");
                output(&mut timing, 50_000, "pause");
            }
            assert_eq!(
                timing.active_pieces(0, 60_000, 60_000),
                vec![(
                    TimeInterval {
                        start: 0,
                        end: 60_000
                    },
                    60_000
                )]
            );
        }
    }

    #[test]
    fn concurrent_tool_execution_and_recorded_reasoning_are_preserved() {
        for structured in [true, false] {
            let mut timing = UserWaitTiming::default();
            call(&mut timing, 10_000, "request_user_input", "question");
            if structured {
                record(
                    &mut timing,
                    40_000,
                    "event_msg",
                    json!({"type":"item_completed","started_at_ms":20_000,"completed_at_ms":40_000,"item":{"type":"Reasoning","id":"reasoning","raw_content":"PRIVATE_REASONING"}}),
                );
            } else {
                call(&mut timing, 20_000, "exec", "work");
                output(&mut timing, 40_000, "work");
            }
            output(&mut timing, 50_000, "question");
            let pieces = timing.active_pieces(0, 60_000, 60_000);
            assert_eq!(pieces.iter().map(|(_, ms)| ms).sum::<u64>(), 40_000);
            assert_eq!(
                pieces[1].0,
                TimeInterval {
                    start: 20_000,
                    end: 40_000
                }
            );
        }
    }

    #[test]
    fn overlapping_questions_are_deducted_once_and_pending_work_is_kept() {
        let mut timing = UserWaitTiming::default();
        call(&mut timing, 10_000, "request_user_input", "first");
        call(&mut timing, 20_000, "request_user_input", "second");
        call(&mut timing, 25_000, "exec", "still-running");
        output(&mut timing, 40_000, "first");
        output(&mut timing, 50_000, "second");
        assert_eq!(
            timing
                .active_pieces(0, 60_000, 60_000)
                .iter()
                .map(|(_, ms)| ms)
                .sum::<u64>(),
            45_000
        );
    }

    #[test]
    fn checkpoint_resumes_pending_question_and_abort_clamps_to_zero() {
        let mut timing = UserWaitTiming::default();
        call(&mut timing, 1000, "request_user_input", "question");
        let encoded = serde_json::to_string(&timing).unwrap();
        let mut restored: UserWaitTiming = serde_json::from_str(&encoded).unwrap();
        output(&mut restored, 9000, "question");
        assert_eq!(
            restored
                .active_pieces(0, 10_000, 10_000)
                .iter()
                .map(|(_, ms)| ms)
                .sum::<u64>(),
            2000
        );
        assert_eq!(
            timing.active_pieces(0, 10_000, 500),
            vec![(
                TimeInterval {
                    start: 0,
                    end: 1000
                },
                0
            )]
        );
        assert_eq!(
            timing.excluded_intervals(),
            vec![TimeInterval {
                start: 1000,
                end: 10_000
            }]
        );
    }

    #[test]
    fn user_wait_item_is_not_mistaken_for_work_and_duration_precision_is_preserved() {
        let mut timing = UserWaitTiming::default();
        call(&mut timing, 1500, "request_user_input", "question");
        record(
            &mut timing,
            6500,
            "event_msg",
            json!({"type":"item_completed","started_at_ms":1500,"completed_at_ms":6500,"item":{"type":"Extension","kind":"functions.request_user_input","id":"question"}}),
        );
        output(&mut timing, 6500, "question");
        let pieces = timing.active_pieces(0, 10_000, 10_123);
        assert_eq!(pieces.iter().map(|(_, ms)| ms).sum::<u64>(), 5123);
        let mut rounded = UserWaitTiming::default();
        assert_eq!(
            rounded.active_pieces(1000, 1000, 123),
            vec![(
                TimeInterval {
                    start: 1000,
                    end: 1000
                },
                123
            )]
        );
        assert_eq!(
            rounded.active_pieces(1000, 1000, 0),
            vec![(
                TimeInterval {
                    start: 1000,
                    end: 1000
                },
                0
            )]
        );
    }
}
