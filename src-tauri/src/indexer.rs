use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

use chrono::Utc;
use chrono_tz::Tz;
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::pricing::PricingCatalog;
use crate::source::{
    ChangePlan, CodexSource, ParsedRecord, SourceCapabilities, SourceChange, SourceCheckpoint,
    SourceKind, StreamOutcome,
};
use crate::store::{SyncRunProgress, UsageStore};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncMode {
    Initial,
    Incremental,
    Rebuild,
    Repair,
}

impl SyncMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Initial => "initial",
            Self::Incremental => "incremental",
            Self::Rebuild => "rebuild",
            Self::Repair => "repair",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncPhase {
    Idle,
    Detecting,
    Planning,
    Importing,
    RollingUp,
    Completed,
    Cancelled,
    Failed,
}

impl SyncPhase {
    fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Detecting => "detecting",
            Self::Planning => "planning",
            Self::Importing => "importing",
            Self::RollingUp => "rolling_up",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub run_id: Option<String>,
    pub mode: Option<SyncMode>,
    pub phase: SyncPhase,
    pub files_total: u64,
    pub files_completed: u64,
    pub bytes_total: u64,
    pub bytes_read: u64,
    pub records_written: u64,
    pub records_skipped: u64,
    pub parse_failures: u64,
    pub file_errors: u64,
    pub error_counts: BTreeMap<String, u64>,
    pub speed_bytes_per_second: u64,
    pub started_at_ms: Option<i64>,
    pub updated_at_ms: i64,
    pub last_completed_at_ms: Option<i64>,
    pub cancel_requested: bool,
    pub capabilities: Option<SourceCapabilities>,
    pub error_code: Option<String>,
}

impl Default for SyncStatus {
    fn default() -> Self {
        Self {
            run_id: None,
            mode: None,
            phase: SyncPhase::Idle,
            files_total: 0,
            files_completed: 0,
            bytes_total: 0,
            bytes_read: 0,
            records_written: 0,
            records_skipped: 0,
            parse_failures: 0,
            file_errors: 0,
            error_counts: BTreeMap::new(),
            speed_bytes_per_second: 0,
            started_at_ms: None,
            updated_at_ms: Utc::now().timestamp_millis(),
            last_completed_at_ms: None,
            cancel_requested: false,
            capabilities: None,
            error_code: None,
        }
    }
}

/// 首次导入、增量、取消、断点和保留策略都封装在这个 Module 的三个方法后面。
pub struct UsageIndexer {
    store: Arc<UsageStore>,
    source: Arc<dyn CodexSource>,
    pricing: Arc<RwLock<PricingCatalog>>,
    timezone: Tz,
    idle_gap_ms: AtomicU64,
    status: RwLock<SyncStatus>,
    cancel: AtomicBool,
    run_guard: Mutex<()>,
}

/// A repair publishes a replacement only after the entire source was read.
/// Returning an error here rolls back the same per-source ingestion transaction.
struct RepairSource<'a>(&'a dyn CodexSource);

impl CodexSource for RepairSource<'_> {
    fn detect(&self) -> AppResult<SourceCapabilities> {
        self.0.detect()
    }

    fn plan(&self, checkpoints: &[SourceCheckpoint]) -> AppResult<ChangePlan> {
        self.0.plan(checkpoints)
    }

    fn stream(
        &self,
        change: &SourceChange,
        sink: &mut dyn FnMut(ParsedRecord) -> AppResult<()>,
        cancel: &AtomicBool,
    ) -> AppResult<StreamOutcome> {
        let outcome = self.0.stream(change, sink, cancel)?;
        if outcome.cancelled || cancel.load(Ordering::Relaxed) {
            return Err(AppError::new("sync_cancelled", "同步已取消"));
        }
        if outcome.parse_failures > 0
            || outcome.incomplete_tail
            || (matches!(
                change.kind(),
                SourceKind::Session | SourceKind::ArchivedSession
            ) && outcome.records_emitted == 0)
        {
            return Err(AppError::new(
                "repair_source_incomplete",
                "来源无法完整读取，已保留此前记录",
            ));
        }
        Ok(outcome)
    }
}

impl UsageIndexer {
    pub fn new(
        store: Arc<UsageStore>,
        source: Arc<dyn CodexSource>,
        pricing: PricingCatalog,
        timezone: Tz,
        idle_gap_ms: u64,
    ) -> Self {
        Self {
            store,
            source,
            pricing: Arc::new(RwLock::new(pricing)),
            timezone,
            idle_gap_ms: AtomicU64::new(idle_gap_ms),
            status: RwLock::new(SyncStatus::default()),
            cancel: AtomicBool::new(false),
            run_guard: Mutex::new(()),
        }
    }

    pub fn status(&self) -> SyncStatus {
        self.status.read().clone()
    }

    pub fn cancel(&self) -> SyncStatus {
        self.cancel.store(true, Ordering::Relaxed);
        let mut status = self.status.write();
        status.cancel_requested = true;
        status.updated_at_ms = Utc::now().timestamp_millis();
        status.clone()
    }

    pub fn replace_pricing(&self, catalog: PricingCatalog) {
        *self.pricing.write() = catalog;
    }

    pub fn set_idle_gap_ms(&self, idle_gap_ms: u64) {
        self.idle_gap_ms.store(idle_gap_ms, Ordering::Relaxed);
    }

    pub fn clear_analysis(&self) -> AppResult<()> {
        let Some(_run_guard) = self.run_guard.try_lock() else {
            return Err(AppError::new(
                "sync_in_progress",
                "请先取消并等待当前同步停止",
            ));
        };
        self.store.clear_analysis()
    }

    pub fn sync(&self, mode: SyncMode) -> AppResult<SyncStatus> {
        let Some(_run_guard) = self.run_guard.try_lock() else {
            return Ok(self.status());
        };
        self.cancel.store(false, Ordering::Relaxed);
        let started_at_ms = Utc::now().timestamp_millis();
        let started = Instant::now();
        let run_id = Uuid::new_v4().to_string();
        self.replace_status(SyncStatus {
            run_id: Some(run_id.clone()),
            mode: Some(mode),
            phase: SyncPhase::Detecting,
            started_at_ms: Some(started_at_ms),
            updated_at_ms: started_at_ms,
            ..SyncStatus::default()
        });

        let prepare = || -> AppResult<()> {
            if mode == SyncMode::Rebuild {
                self.store.reset_index()?;
            } else if mode == SyncMode::Repair && !self.store.integrity_check()? {
                // Page-level corruption cannot safely be mapped to sessions.
                // Never turn a selective repair into a destructive full reset.
                return Err(AppError::new(
                    "repair_database_damaged",
                    "数据库结构损坏，已停止修复并保留现有数据",
                ));
            }
            Ok(())
        };
        prepare().inspect_err(|error| self.fail_status(&error.code))?;

        let capabilities = self.source.detect().inspect_err(|_| {
            self.fail_status("source_detection_failed");
        })?;
        self.update_status(|status| {
            status.capabilities = Some(capabilities);
            status.phase = SyncPhase::Planning;
        });
        let checkpoints = self
            .store
            .sync_checkpoints(mode == SyncMode::Repair)
            .inspect_err(|_| {
                self.fail_status("checkpoint_read_failed");
            })?;
        let plan = self.source.plan(&checkpoints).inspect_err(|_| {
            self.fail_status("change_plan_failed");
        })?;
        self.update_status(|status| {
            status.files_total = plan.changes.len() as u64;
            status.bytes_total = plan.total_bytes;
            for (agent, code) in &plan.source_errors {
                status.file_errors += 1;
                status.records_skipped += 1;
                *status
                    .error_counts
                    .entry(format!("{agent}:{code}"))
                    .or_default() += 1;
            }
        });
        self.store.begin_sync_run(
            &run_id,
            mode.as_str(),
            plan.changes.len() as u64,
            plan.total_bytes,
            started_at_ms,
        )?;

        let repair_source = RepairSource(self.source.as_ref());
        let source: &dyn CodexSource = if mode == SyncMode::Repair {
            &repair_source
        } else {
            self.source.as_ref()
        };
        let mut displaced_sessions = HashSet::new();
        for change in &plan.changes {
            if self.cancel.load(Ordering::Relaxed) {
                break;
            }
            self.update_status(|status| status.phase = SyncPhase::Importing);
            let result = (|| {
                if mode == SyncMode::Repair {
                    self.store.ensure_repairable_source(change)?;
                }
                self.store.apply_source_change(
                    source,
                    change,
                    &self.pricing.read(),
                    self.timezone,
                    self.idle_gap_ms.load(Ordering::Relaxed),
                    &self.cancel,
                )
            })();
            match result {
                Ok(result) => {
                    if let Some(session_id) = result.displaced_session_id {
                        displaced_sessions.insert(session_id);
                    }
                    self.update_status(|status| {
                        status.files_completed = status.files_completed.saturating_add(1);
                        status.bytes_read = status.bytes_read.saturating_add(result.bytes_read);
                        status.records_written = status
                            .records_written
                            .saturating_add(result.records_written);
                        status.parse_failures =
                            status.parse_failures.saturating_add(result.parse_failures);
                    });
                    if result.cancelled {
                        self.cancel.store(true, Ordering::Relaxed);
                        break;
                    }
                }
                Err(error) if error.code == "sync_cancelled" => {
                    self.cancel.store(true, Ordering::Relaxed);
                    break;
                }
                Err(error) => {
                    let _ = self
                        .store
                        .mark_source_error(change.source_key(), &error.code);
                    self.update_status(|status| {
                        status.files_completed = status.files_completed.saturating_add(1);
                        status.file_errors = status.file_errors.saturating_add(1);
                        status.records_skipped = status.records_skipped.saturating_add(1);
                        let key =
                            format!("{:?}:{:?}:{}", change.kind(), change.action(), error.code);
                        *status.error_counts.entry(key).or_default() += 1;
                    });
                }
            }
            self.refresh_speed(started);
            self.persist_progress(&run_id, started, "running", None)?;
        }

        for session_id in displaced_sessions {
            self.store.rebuild_session_derived(
                &session_id,
                self.timezone,
                self.idle_gap_ms.load(Ordering::Relaxed),
            )?;
        }
        self.update_status(|status| status.phase = SyncPhase::RollingUp);
        self.store
            .rebuild_rollups_and_prune(Utc::now().timestamp_millis(), 90, self.timezone)?;

        let cancelled = self.cancel.load(Ordering::Relaxed);
        let finished_at_ms = Utc::now().timestamp_millis();
        self.update_status(|status| {
            status.phase = if cancelled {
                SyncPhase::Cancelled
            } else {
                SyncPhase::Completed
            };
            status.cancel_requested = cancelled;
            status.last_completed_at_ms = (!cancelled).then_some(finished_at_ms);
            status.updated_at_ms = finished_at_ms;
        });
        self.refresh_speed(started);
        let database_status = if cancelled { "cancelled" } else { "completed" };
        self.persist_finished(&run_id, started, database_status, finished_at_ms)?;
        Ok(self.status())
    }

    fn replace_status(&self, next: SyncStatus) {
        *self.status.write() = next;
    }

    fn update_status(&self, operation: impl FnOnce(&mut SyncStatus)) {
        let mut status = self.status.write();
        operation(&mut status);
        status.updated_at_ms = Utc::now().timestamp_millis();
    }

    fn refresh_speed(&self, started: Instant) {
        self.update_status(|status| {
            let elapsed = started.elapsed().as_secs_f64().max(0.001);
            status.speed_bytes_per_second = (status.bytes_read as f64 / elapsed) as u64;
        });
    }

    fn fail_status(&self, error_code: &str) {
        self.update_status(|status| {
            status.phase = SyncPhase::Failed;
            status.error_code = Some(error_code.to_string());
        });
    }

    fn persist_progress(
        &self,
        run_id: &str,
        started: Instant,
        database_status: &str,
        error_code: Option<&str>,
    ) -> AppResult<()> {
        let status = self.status();
        self.store.update_sync_run(&SyncRunProgress {
            id: run_id,
            status: database_status,
            stage: status.phase.as_str(),
            files_completed: status.files_completed,
            bytes_read: status.bytes_read,
            events_written: status.records_written,
            records_skipped: status.records_skipped,
            parse_failures: status.parse_failures,
            elapsed_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
            error_code,
        })
    }

    fn persist_finished(
        &self,
        run_id: &str,
        started: Instant,
        database_status: &str,
        finished_at_ms: i64,
    ) -> AppResult<()> {
        let status = self.status();
        self.store.finish_sync_run(
            &SyncRunProgress {
                id: run_id,
                status: database_status,
                stage: status.phase.as_str(),
                files_completed: status.files_completed,
                bytes_read: status.bytes_read,
                events_written: status.records_written,
                records_skipped: status.records_skipped,
                parse_failures: status.parse_failures,
                elapsed_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
                error_code: status.error_code.as_deref(),
            },
            finished_at_ms,
        )
    }
}

pub fn system_timezone() -> Tz {
    iana_time_zone::get_timezone()
        .ok()
        .and_then(|name| name.parse().ok())
        .unwrap_or(chrono_tz::UTC)
}

pub fn default_codex_root() -> AppResult<std::path::PathBuf> {
    dirs::home_dir()
        .map(|home| home.join(".codex"))
        .ok_or_else(|| AppError::new("home_unavailable", "无法确定用户主目录"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::FsCodexSource;

    struct CancelAfterRecord(FsCodexSource);

    impl CodexSource for CancelAfterRecord {
        fn detect(&self) -> AppResult<SourceCapabilities> {
            self.0.detect()
        }

        fn plan(&self, checkpoints: &[SourceCheckpoint]) -> AppResult<ChangePlan> {
            self.0.plan(checkpoints)
        }

        fn stream(
            &self,
            change: &SourceChange,
            sink: &mut dyn FnMut(ParsedRecord) -> AppResult<()>,
            cancel: &AtomicBool,
        ) -> AppResult<StreamOutcome> {
            self.0.stream(
                change,
                &mut |record| {
                    sink(record)?;
                    cancel.store(true, Ordering::Relaxed);
                    Ok(())
                },
                cancel,
            )
        }
    }

    #[test]
    fn cancelled_codex_repair_keeps_the_previous_snapshot() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("codex");
        std::fs::create_dir_all(root.join("sessions")).unwrap();
        let now = Utc::now().to_rfc3339();
        let lines = [
            serde_json::json!({"timestamp":now,"type":"session_meta","payload":{"id":"repair-cancel","cwd":"C:/repo","model_provider":"openai"}}),
            serde_json::json!({"timestamp":now,"type":"turn_context","payload":{"model":"gpt-6.1-sol"}}),
            serde_json::json!({"timestamp":now,"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"output_tokens":20}}}}),
        ].map(|value| format!("{value}\n")).concat();
        std::fs::write(root.join("sessions/rollout-repair.jsonl"), lines).unwrap();
        let store = Arc::new(UsageStore::open(directory.path().join("usage.sqlite3")).unwrap());
        let initial = UsageIndexer::new(
            store.clone(),
            Arc::new(FsCodexSource::new(&root)),
            PricingCatalog::default(),
            chrono_tz::UTC,
            30 * 60 * 1000,
        );
        assert_eq!(initial.sync(SyncMode::Initial).unwrap().file_errors, 0);
        store
            .with_writer(|transaction| {
                transaction.execute("UPDATE source_files SET status='error'", [])?;
                Ok(())
            })
            .unwrap();
        let repair = UsageIndexer::new(
            store.clone(),
            Arc::new(CancelAfterRecord(FsCodexSource::new(root))),
            PricingCatalog::default(),
            chrono_tz::UTC,
            30 * 60 * 1000,
        );
        let status = repair.sync(SyncMode::Repair).unwrap();
        assert_eq!(status.phase, SyncPhase::Cancelled);
        store
            .with_reader(|connection| {
                let tokens: i64 = connection.query_row(
                    "SELECT total_tokens FROM sessions WHERE id='repair-cancel'",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(tokens, 120);
                let tokens: i64 = connection.query_row(
                    "SELECT SUM(total_tokens) FROM usage_events",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(tokens, 120);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn clearing_analysis_shares_the_sync_lock() {
        let store = Arc::new(UsageStore::open_in_memory().unwrap());
        let indexer = UsageIndexer::new(
            store,
            Arc::new(FsCodexSource::new("unused")),
            PricingCatalog::default(),
            chrono_tz::UTC,
            30 * 60 * 1000,
        );
        let _running = indexer.run_guard.lock();
        assert_eq!(
            indexer.clear_analysis().unwrap_err().code,
            "sync_in_progress"
        );
    }

    #[test]
    fn system_timezone_always_has_a_safe_fallback() {
        let timezone = system_timezone();
        assert!(!timezone.name().is_empty());
    }
}
