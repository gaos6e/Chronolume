//! Audited vendor snapshots. Rate brackets are separate pricing IDs so permanent
//! summaries retain the request's bracket after event retention and repricing.
use std::sync::OnceLock;

use chrono::{Datelike, Timelike, Utc};
use serde::Deserialize;

use super::{BUILTIN_PRICING_REVISION, ModelPrice, PriceableTokens};

#[derive(Deserialize)]
pub(super) struct Rates {
    input: String,
    output: String,
    read: String,
    write: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Tier {
    pub suffix: String,
    pub label: String,
    rates: Rates,
    min_input_tokens: Option<u64>,
    starts_at_ms: Option<i64>,
    #[serde(default)]
    off_peak: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct OfficialModel {
    pub provider: String,
    pub id: String,
    pub source_url: String,
    pub note: String,
    pub aliases: Vec<String>,
    rates: Rates,
    pub tiers: Vec<Tier>,
}

pub(super) fn models() -> &'static [OfficialModel] {
    static MODELS: OnceLock<Vec<OfficialModel>> = OnceLock::new();
    MODELS.get_or_init(|| {
        serde_json::from_str(include_str!("official-prices.json"))
            .expect("audited official pricing snapshot")
    })
}

pub(super) fn model(provider: &str, id: &str) -> Option<&'static OfficialModel> {
    let base = id.split('@').next().unwrap_or(id);
    models()
        .iter()
        .find(|m| m.provider == provider && m.id == base)
}

pub(super) fn alias(id: &str) -> Option<String> {
    models()
        .iter()
        .find(|m| m.aliases.iter().any(|a| a == id))
        .map(|m| m.id.clone())
}

pub(super) fn prices() -> Vec<ModelPrice> {
    let mut prices = Vec::new();
    for model in models() {
        for (id, rates) in std::iter::once((model.id.clone(), &model.rates)).chain(
            model
                .tiers
                .iter()
                .map(|t| (format!("{}@{}", model.id, t.suffix), &t.rates)),
        ) {
            prices.push(ModelPrice {
                provider: model.provider.clone(),
                pricing_id: id,
                input_per_million_usd: rates.input.parse().expect("official input price"),
                output_per_million_usd: rates.output.parse().expect("official output price"),
                cache_read_per_million_usd: rates.read.parse().expect("official cached price"),
                cache_write_per_million_usd: rates
                    .write
                    .as_ref()
                    .map(|v| v.parse().expect("official write price")),
                revision: BUILTIN_PRICING_REVISION,
            });
        }
    }
    prices
}

pub(super) fn tier_id(
    provider: &str,
    id: &str,
    tokens: PriceableTokens,
    at: Option<i64>,
) -> Option<String> {
    let model = model(provider, id)?;
    let input = tokens
        .fresh_input
        .saturating_add(tokens.cached_input)
        .saturating_add(tokens.cache_write);
    model
        .tiers
        .iter()
        .find(|tier| {
            tier.min_input_tokens.is_none_or(|min| input >= min)
                && tier
                    .starts_at_ms
                    .is_none_or(|start| at.is_some_and(|time| time >= start))
                && (!tier.off_peak || at.is_some_and(deepseek_off_peak))
        })
        .map(|tier| format!("{}@{}", model.id, tier.suffix))
}

fn deepseek_off_peak(at: i64) -> bool {
    let Some(time) = chrono::DateTime::<Utc>::from_timestamp_millis(at) else {
        return false;
    };
    // Calendar snapshot: 国办发明电〔2025〕7号. Weekend make-up work days
    // remain off-peak because DeepSeek's published rule is Monday–Friday.
    let date = time.with_timezone(&chrono_tz::Asia::Shanghai);
    let holiday = date.year() == 2026
        && matches!(
            (date.month(), date.day()),
            (1, 1..=3)
                | (2, 15..=23)
                | (4, 4..=6)
                | (5, 1..=5)
                | (6, 19..=21)
                | (9, 25..=27)
                | (10, 1..=7)
        );
    time.weekday().number_from_monday() > 5 || holiday || !matches!(time.hour(), 1..=3 | 6..=9)
}

pub fn source(provider: &str) -> &'static str {
    match provider {
        "anthropic" => super::ANTHROPIC_PRICING_SOURCE,
        "deepseek" => "https://api-docs.deepseek.com/quick_start/pricing/",
        "google" => "https://ai.google.dev/gemini-api/docs/pricing",
        "zai" => "https://docs.z.ai/guides/overview/pricing",
        "minimax" => "https://platform.minimax.io/docs/pricing/overview",
        "moonshotai" => "https://platform.kimi.ai/",
        "alibaba" => "https://www.alibabacloud.com/help/en/model-studio/model-pricing",
        "xai" => "https://docs.x.ai/developers/pricing",
        "opencode" => "https://opencode.ai/v2/docs/console/models/",
        _ => super::OFFICIAL_PRICING_SOURCE,
    }
}

pub fn price_source(provider: &str, id: &str) -> &'static str {
    model(provider, id).map_or_else(|| source(provider), |model| model.source_url.as_str())
}

pub fn description(provider: &str, id: &str) -> Option<String> {
    let model = model(provider, id)?;
    let bracket = id
        .split_once('@')
        .and_then(|(_, suffix)| model.tiers.iter().find(|t| t.suffix == suffix));
    Some(bracket.map_or_else(
        || model.note.clone(),
        |t| format!("{} · {}", t.label, model.note),
    ))
}

/// This identifies a vendor only; a price still requires an exact catalog match.
pub(super) fn vendor(id: &str) -> Option<&'static str> {
    if id.starts_with("gpt-")
        || id.starts_with("o1")
        || id.starts_with("o3")
        || id.starts_with("o4")
    {
        Some("openai")
    } else if id.starts_with("claude-") {
        Some("anthropic")
    } else if id.starts_with("deepseek-") && !id.ends_with("-free") {
        Some("deepseek")
    } else if id.starts_with("gemini-") {
        Some("google")
    } else if id.starts_with("glm-") && !id.ends_with("-free") {
        Some("zai")
    } else if id.starts_with("minimax-") {
        Some("minimax")
    } else if id.starts_with("kimi-") {
        Some("moonshotai")
    } else if id.starts_with("qwen") {
        Some("alibaba")
    } else if id.starts_with("grok-") {
        Some("xai")
    } else {
        None
    }
}

pub(super) fn provider_alias(provider: &str) -> &str {
    match provider {
        "deepseek-official" => "deepseek",
        "google-generative-ai" | "google-vertex" => "google",
        "z-ai" | "zhipu" => "zai",
        "moonshot" | "kimi" => "moonshotai",
        "dashscope" | "qwen" => "alibaba",
        "x-ai" => "xai",
        "opencode-zen" => "opencode",
        _ => provider,
    }
}
