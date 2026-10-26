use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use super::{
    ChangeAction, ChangePlan, FsCodexSource, PARSER_VERSION, ParsedRecord, SourceCapabilities,
    SourceChange, SourceCheckpoint, SourceCursor, SourceKind, SourceStatus, StreamOutcome,
    UsageSource, modified_ns, normalize_relative_path, prefix_hash,
};
use crate::error::{AppError, AppResult};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SourceSettings {
    pub codex: Option<String>,
    pub claude: Option<String>,
    pub opencode: Option<String>,
    pub dsh: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSourceSetting {
    pub agent: String,
    pub path: String,
    pub custom_path: Option<String>,
    pub available: bool,
}

impl SourceSettings {
    pub fn validate(&mut self) -> AppResult<()> {
        for value in [
            &mut self.codex,
            &mut self.claude,
            &mut self.opencode,
            &mut self.dsh,
        ] {
            *value = value
                .take()
                .map(|path| path.trim().to_string())
                .filter(|path| !path.is_empty());
            if value
                .as_deref()
                .is_some_and(|path| !Path::new(path).is_absolute())
            {
                return Err(AppError::new(
                    "invalid_source_path",
                    "数据目录必须使用绝对路径",
                ));
            }
        }
        Ok(())
    }

    pub fn resolved(&self, agent: &str) -> AppResult<PathBuf> {
        let home = dirs::home_dir()
            .ok_or_else(|| AppError::new("home_unavailable", "无法确定用户主目录"))?;
        let environment = |name: &str| {
            std::env::var_os(name)
                .filter(|path| !path.is_empty())
                .map(PathBuf::from)
        };
        Ok(match agent {
            "codex" => self
                .codex
                .as_ref()
                .map(PathBuf::from)
                .or_else(|| environment("CODEX_HOME"))
                .unwrap_or_else(|| home.join(".codex")),
            "claude" => self
                .claude
                .as_ref()
                .map(PathBuf::from)
                .or_else(|| environment("CLAUDE_CONFIG_DIR"))
                .unwrap_or_else(|| home.join(".claude")),
            "dsh" => self
                .dsh
                .as_ref()
                .map(PathBuf::from)
                .or_else(|| environment("DSH_HOME"))
                .unwrap_or_else(|| home.join(".dsh")),
            "opencode" => {
                if let Some(path) = &self.opencode {
                    return Ok(PathBuf::from(path));
                }
                let data = environment("OPENCODE_DATA_DIR").unwrap_or_else(|| {
                    environment("XDG_DATA_HOME")
                        .unwrap_or_else(|| home.join(".local/share"))
                        .join("opencode")
                });
                if let Some(db) = environment("OPENCODE_DB") {
                    if db == Path::new(":memory:") {
                        return Ok(data.join(".memory-sessions-unavailable"));
                    }
                    if db.is_absolute() { db } else { data.join(db) }
                } else {
                    data
                }
            }
            _ => return Err(AppError::new("unknown_agent", "未知数据来源")),
        })
    }

    pub fn describe(&self) -> AppResult<Vec<AgentSourceSetting>> {
        ["codex", "claude", "opencode", "dsh"]
            .into_iter()
            .map(|agent| {
                let path = self.resolved(agent)?;
                let custom_path = match agent {
                    "codex" => &self.codex,
                    "claude" => &self.claude,
                    "dsh" => &self.dsh,
                    _ => &self.opencode,
                }
                .clone();
                let available = match agent {
                    "codex" => {
                        path.join("sessions").is_dir() || path.join("state_5.sqlite").is_file()
                    }
                    "claude" => path.join("projects").is_dir(),
                    "dsh" => path.join("sessions").is_dir() || path.is_dir(),
                    _ => {
                        path.is_file()
                            || path.join("storage").is_dir()
                            || super::opencode::databases(&path)
                                .is_ok_and(|paths| !paths.is_empty())
                    }
                };
                Ok(AgentSourceSetting {
                    agent: agent.into(),
                    path: path.to_string_lossy().into_owned(),
                    custom_path,
                    available,
                })
            })
            .collect()
    }
}

pub struct MultiAgentSource {
    settings: RwLock<SourceSettings>,
}
impl MultiAgentSource {
    pub fn new(settings: SourceSettings) -> Self {
        Self {
            settings: RwLock::new(settings),
        }
    }
    pub fn configure(&self, settings: SourceSettings) {
        *self.settings.write() = settings;
    }
    pub fn settings(&self) -> SourceSettings {
        self.settings.read().clone()
    }

    fn discover(&self) -> AppResult<ChangePlan> {
        let settings = self.settings.read();
        let mut plan = ChangePlan::default();
        for agent in ["claude", "opencode", "dsh"] {
            let root = settings.resolved(agent)?;
            let discovery = match agent {
                "claude" => claude_files(&root),
                "dsh" => super::dsh::discover(&root),
                _ => super::opencode::discover(&root),
            };
            match discovery {
                Ok(changes) => plan.changes.extend(changes),
                Err(error) => plan.source_errors.push((agent.into(), error.code)),
            }
        }
        Ok(plan)
    }
}

impl UsageSource for MultiAgentSource {
    fn detect(&self) -> AppResult<SourceCapabilities> {
        let settings = self.settings();
        // Detection is a capability snapshot. Planning reports source failures
        // independently, so one unreadable agent does not block the others.
        let mut capabilities = FsCodexSource::new(settings.resolved("codex")?)
            .detect()
            .unwrap_or(SourceCapabilities {
                statuses: Vec::new(),
                can_read_session_tokens: false,
                can_read_session_metadata: false,
                can_read_logs: false,
            });
        let discovery = self.discover()?;
        for setting in settings
            .describe()?
            .into_iter()
            .filter(|s| s.agent != "codex")
        {
            let matching: Vec<_> = discovery
                .changes
                .iter()
                .filter(|file| file.source_key.starts_with(&format!("{}:", setting.agent)))
                .collect();
            capabilities.can_read_session_tokens |= !matching.is_empty();
            capabilities.can_read_session_metadata |= !matching.is_empty();
            capabilities.statuses.push(SourceStatus {
                agent: setting.agent,
                kind: SourceKind::Session,
                exists: setting.available || !matching.is_empty(),
                file_count: matching.len() as u64,
                total_bytes: matching.iter().map(|file| file.file_size).sum(),
            });
        }
        Ok(capabilities)
    }

    fn plan(&self, checkpoints: &[SourceCheckpoint]) -> AppResult<ChangePlan> {
        let is_external = |key: &str| {
            ["claude:", "opencode:", "dsh:"]
                .iter()
                .any(|prefix| key.starts_with(prefix))
        };
        let codex_checkpoints: Vec<_> = checkpoints
            .iter()
            .filter(|c| !is_external(&c.source_key))
            .cloned()
            .collect();
        let mut plan = match FsCodexSource::new(self.settings.read().resolved("codex")?)
            .plan(&codex_checkpoints)
        {
            Ok(plan) => plan,
            Err(error) => ChangePlan {
                source_errors: vec![("codex".into(), error.code)],
                ..ChangePlan::default()
            },
        };
        let discovery = self.discover()?;
        plan.source_errors.extend(discovery.source_errors);
        let previous: HashMap<_, _> = checkpoints
            .iter()
            .filter(|c| is_external(&c.source_key))
            .map(|c| (c.source_key.as_str(), c))
            .collect();
        let mut seen = std::collections::HashSet::new();
        for change in discovery.changes {
            seen.insert(change.source_key.clone());
            let unchanged = previous.get(change.source_key.as_str()).is_some_and(|old| {
                old.file_size == change.file_size
                    && old.mtime_ns == change.mtime_ns
                    && old.prefix_hash == change.prefix_hash
                    && old.parser_version == PARSER_VERSION
            });
            if !unchanged {
                plan.changes.push(change);
            }
        }
        for checkpoint in previous.values().filter(|c| {
            !seen.contains(&c.source_key)
                && !plan
                    .source_errors
                    .iter()
                    .any(|(agent, _)| c.source_key.starts_with(&format!("{agent}:")))
        }) {
            plan.changes.push(SourceChange {
                source_key: checkpoint.source_key.clone(),
                relative_path: checkpoint.relative_path.clone(),
                path: PathBuf::new(),
                kind: checkpoint.kind,
                session_id: checkpoint.session_id.clone(),
                action: ChangeAction::Missing,
                file_size: 0,
                mtime_ns: 0,
                prefix_hash: checkpoint.prefix_hash.clone(),
                cursor: checkpoint.cursor.clone(),
            });
        }
        plan.total_bytes = plan.changes.iter().map(SourceChange::expected_bytes).sum();
        Ok(plan)
    }

    fn stream(
        &self,
        change: &SourceChange,
        sink: &mut dyn FnMut(ParsedRecord) -> AppResult<()>,
        cancel: &AtomicBool,
    ) -> AppResult<StreamOutcome> {
        if matches!(
            change.action,
            ChangeAction::Missing | ChangeAction::MetadataOnly
        ) {
            return FsCodexSource::new(PathBuf::new()).stream(change, sink, cancel);
        }
        if change.source_key.starts_with("claude:") {
            super::claude::stream(change, sink, cancel)
        } else if change.source_key.starts_with("opencode:") {
            super::opencode::stream(change, sink, cancel)
        } else if change.source_key.starts_with("dsh:") {
            super::dsh::stream(change, sink, cancel)
        } else {
            FsCodexSource::new(self.settings.read().resolved("codex")?).stream(change, sink, cancel)
        }
    }
}

fn claude_files(root: &Path) -> AppResult<Vec<SourceChange>> {
    let projects = root.join("projects");
    if !projects.is_dir() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    for entry in WalkDir::new(&projects).follow_links(false) {
        let entry = entry.map_err(|_| AppError::filesystem())?;
        if !entry.file_type().is_file()
            || entry.path().extension().and_then(|s| s.to_str()) != Some("jsonl")
        {
            continue;
        }
        let stem = entry
            .path()
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        if stem == "journal" || stem.contains(".orphaned-") {
            continue;
        }
        let id = if stem.starts_with("agent-") {
            let nested_parent = entry
                .path()
                .ancestors()
                .find(|path| path.file_name().is_some_and(|name| name == "subagents"))
                .and_then(Path::parent)
                .and_then(Path::file_name)
                .and_then(|name| name.to_str())
                .map(str::to_owned);
            let parent = nested_parent.or_else(|| {
                let file = File::open(entry.path()).ok()?;
                for line in BufReader::new(file).lines().take(32).flatten() {
                    #[derive(Deserialize)]
                    struct Identity {
                        #[serde(rename = "sessionId")]
                        session_id: Option<String>,
                    }
                    if let Ok(Identity {
                        session_id: Some(id),
                    }) = serde_json::from_str::<Identity>(&line)
                    {
                        return Some(id);
                    }
                }
                None
            });
            match parent {
                Some(parent) => format!(
                    "claude:{parent}:agent:{}",
                    stem.trim_start_matches("agent-")
                ),
                None => format!("claude:{stem}"),
            }
        } else {
            format!("claude:{stem}")
        };
        let metadata = fs::metadata(entry.path())?;
        files.push(SourceChange {
            source_key: id.clone(),
            session_id: Some(id),
            path: entry.path().to_path_buf(),
            relative_path: normalize_relative_path(
                entry
                    .path()
                    .strip_prefix(root)
                    .map_err(|_| AppError::filesystem())?,
            ),
            kind: SourceKind::Session,
            action: ChangeAction::Replay,
            file_size: metadata.len(),
            mtime_ns: modified_ns(&metadata),
            prefix_hash: prefix_hash(entry.path())?,
            cursor: SourceCursor::default(),
        });
    }
    // A copied transcript with the same session ID is one logical source.
    files.sort_by(|a, b| {
        a.source_key
            .cmp(&b.source_key)
            .then_with(|| b.file_size.cmp(&a.file_size))
            .then_with(|| a.relative_path.cmp(&b.relative_path))
    });
    files.dedup_by(|a, b| a.source_key == b.source_key);
    Ok(files)
}
