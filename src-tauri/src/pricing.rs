use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::OnceLock;

pub mod official;

pub const OFFICIAL_PRICING_SOURCE: &str = "https://developers.openai.com/api/docs/pricing";
pub const BUILTIN_PRICING_REVISION: i64 = 2_026_112_101;
pub const ANTHROPIC_PRICING_SOURCE: &str =
    "https://platform.claude.com/docs/en/about-claude/pricing";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedModelId {
    pub exact: String,
    pub candidates: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPrice {
    pub provider: String,
    pub pricing_id: String,
    pub input_per_million_usd: Decimal,
    pub output_per_million_usd: Decimal,
    pub cache_read_per_million_usd: Decimal,
    pub cache_write_per_million_usd: Option<Decimal>,
    pub revision: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PriceableTokens {
    /// Subset of cache_write charged at the vendor's one-hour rate.
    pub cache_write_1h: u64,
    pub fresh_input: u64,
    pub cached_input: u64,
    pub output: u64,
    pub cache_write: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PriceQuote {
    Priced {
        pricing_id: String,
        revision: i64,
        total_microusd: i64,
    },
    /// 内部功能产生的 Token 保留统计，但既不猜价，也不计入缺价告警。
    Excluded,
    Unpriced,
}

#[derive(Debug, Clone, Default)]
pub struct PricingCatalog {
    prices: HashMap<String, ModelPrice>,
    overrides: HashSet<String>,
}

pub fn builtin_model_prices() -> Vec<ModelPrice> {
    let mut prices: Vec<_> = [
        ("gpt-6.1-sol", "2", "10", "0.1", Some("2.5")),
        ("gpt-6.1-terra", "1", "5", "0.05", Some("1.25")),
        ("gpt-6-astra", "10", "50", "1", Some("12.5")),
        ("gpt-6-sol", "2", "10", "0.2", Some("2.5")),
        ("gpt-6-luna", "0.1", "0.5", "0.01", Some("0.125")),
        ("gpt-5.6-sol", "4", "20", "0.4", Some("5")),
        ("gpt-5.6-terra", "2", "12", "0.2", Some("2.5")),
        ("gpt-5.6-luna", "0.2", "1.2", "0.02", Some("0.25")),
        ("gpt-5.5", "5", "30", "0.5", None),
        ("gpt-5.4", "2.5", "15", "0.25", None),
        ("gpt-5.4-mini", "0.75", "4.5", "0.075", None),
        ("gpt-5.4-nano", "0.2", "1.25", "0.02", None),
        ("gpt-5.3-codex", "1.75", "14", "0.175", None),
        ("gpt-5.2", "1.75", "14", "0.175", None),
        ("gpt-5.1", "1.25", "10", "0.125", None),
        ("gpt-5", "1.25", "10", "0.125", None),
        ("gpt-5-mini", "0.25", "2", "0.025", None),
        ("gpt-5-nano", "0.05", "0.4", "0.005", None),
        ("gpt-4.1", "2", "8", "0.5", None),
        ("gpt-4.1-mini", "0.4", "1.6", "0.1", None),
        ("gpt-4.1-nano", "0.1", "0.4", "0.025", None),
        ("gpt-4o", "2.5", "10", "1.25", None),
        ("gpt-4o-mini", "0.15", "0.6", "0.075", None),
        ("o3", "2", "8", "0.5", None),
        ("o4-mini", "1.1", "4.4", "0.275", None),
        ("o3-mini", "1.1", "4.4", "0.55", None),
        ("o1", "15", "60", "7.5", None),
        ("o1-mini", "1.1", "4.4", "0.55", None),
    ]
    .into_iter()
    .map(|(id, input, output, cache_read, cache_write)| ModelPrice {
        provider: "openai".to_string(),
        pricing_id: id.to_string(),
        input_per_million_usd: Decimal::from_str(input).expect("built-in input price"),
        output_per_million_usd: Decimal::from_str(output).expect("built-in output price"),
        cache_read_per_million_usd: Decimal::from_str(cache_read)
            .expect("built-in cache-read price"),
        cache_write_per_million_usd: cache_write
            .map(|value| Decimal::from_str(value).expect("built-in cache-write price")),
        revision: BUILTIN_PRICING_REVISION,
    })
    .collect();
    // Standard, global API pricing snapshot checked 2026-10-26. One-hour writes
    // use 2x input; longer-context/fast/region surcharges are outside this estimate.
    prices.extend(
        [
            ("claude-fable-5-1", "10", "50", "0.25"),
            ("claude-mythos-5-1", "10", "50", "0.25"),
            ("claude-opus-5-5", "4", "20", "0.20"),
            ("claude-sonnet-5-5", "2", "10", "0.10"),
            ("claude-haiku-5-5", "0.1", "0.5", "0.01"),
            ("claude-fable-5", "10", "50", "1"),
            ("claude-mythos-5", "10", "50", "1"),
            ("claude-opus-5", "5", "25", "0.5"),
            ("claude-opus-4-8", "5", "25", "0.5"),
            ("claude-opus-4-7", "5", "25", "0.5"),
            ("claude-opus-4-6", "5", "25", "0.5"),
            ("claude-opus-4-5", "5", "25", "0.5"),
            ("claude-opus-4-1", "15", "75", "1.5"),
            ("claude-opus-4", "15", "75", "1.5"),
            ("claude-sonnet-5", "2", "10", "0.2"),
            ("claude-sonnet-4-6", "3", "15", "0.3"),
            ("claude-sonnet-4-5", "3", "15", "0.3"),
            ("claude-sonnet-4", "3", "15", "0.3"),
            ("claude-haiku-4-5", "1", "5", "0.1"),
            ("claude-3-5-haiku", "0.8", "4", "0.08"),
        ]
        .into_iter()
        .map(|(id, input, output, read)| {
            let input = Decimal::from_str(input).expect("built-in Anthropic input price");
            ModelPrice {
                provider: "anthropic".into(),
                pricing_id: id.into(),
                input_per_million_usd: input,
                output_per_million_usd: Decimal::from_str(output)
                    .expect("built-in Anthropic output price"),
                cache_read_per_million_usd: Decimal::from_str(read)
                    .expect("built-in Anthropic cache price"),
                cache_write_per_million_usd: Some(input * Decimal::new(125, 2)),
                revision: BUILTIN_PRICING_REVISION,
            }
        }),
    );
    for price in official::prices() {
        prices.retain(|existing| {
            existing.provider != price.provider || existing.pricing_id != price.pricing_id
        });
        prices.push(price);
    }
    prices
}

impl PricingCatalog {
    pub fn new(prices: Vec<ModelPrice>) -> Self {
        Self {
            prices: prices
                .into_iter()
                .map(|price| (price.pricing_id.clone(), price))
                .collect(),
            overrides: HashSet::new(),
        }
    }

    pub fn with_overrides(mut self, overrides: HashSet<String>) -> Self {
        self.overrides = overrides;
        self
    }

    pub(crate) fn has_price(&self, pricing_id: &str) -> bool {
        self.prices.contains_key(pricing_id)
    }

    pub fn quote(&self, provider: &str, raw_model: &str, tokens: PriceableTokens) -> PriceQuote {
        self.quote_request(provider, raw_model, tokens, None, None)
    }

    pub fn quote_at(
        &self,
        provider: &str,
        raw_model: &str,
        tokens: PriceableTokens,
        at_ms: i64,
    ) -> PriceQuote {
        self.quote_request(provider, raw_model, tokens, Some(at_ms), None)
    }

    /// Aggregated summaries already carry their request bracket; never choose a
    /// context tier from a whole day's or segment's combined input tokens.
    pub fn quote_summary(
        &self,
        provider: &str,
        raw_model: &str,
        tokens: PriceableTokens,
        stored_id: &str,
    ) -> PriceQuote {
        self.quote_request(provider, raw_model, tokens, None, Some(stored_id))
    }

    fn quote_request(
        &self,
        provider: &str,
        raw_model: &str,
        tokens: PriceableTokens,
        at: Option<i64>,
        stored_id: Option<&str>,
    ) -> PriceQuote {
        let normalized = normalize_model_id(raw_model);
        if is_cost_excluded(provider, &normalized.exact) {
            return PriceQuote::Excluded;
        }
        if matches!(normalized.exact.as_str(), "" | "unknown") {
            return PriceQuote::Unpriced;
        }
        let Some(mut price) = normalized
            .candidates
            .iter()
            .find_map(|candidate| self.prices.get(candidate))
        else {
            return PriceQuote::Unpriced;
        };
        let overridden = self.overrides.contains(&price.pricing_id);
        let tier = if let Some(stored) = stored_id {
            stored
                .split_once('@')
                .filter(|(base, _)| canonical_model_name(base) == price.pricing_id)
                .map(|(_, suffix)| format!("{}@{suffix}", price.pricing_id))
        } else {
            official::tier_id(&price.provider, &price.pricing_id, tokens, at)
        };
        // Keep the original request bracket even while a flat user override is
        // active, so restoring official prices after retention remains lossless.
        let accounting_id = tier.clone().unwrap_or_else(|| price.pricing_id.clone());
        if !overridden {
            if let Some(tier) = tier {
                let Some(bracket) = self.prices.get(&tier) else {
                    return PriceQuote::Unpriced;
                };
                price = bracket;
            }
        }
        if tokens.cache_write_1h > tokens.cache_write
            || (tokens.cached_input > 0
                && !self.overrides.contains(&price.pricing_id)
                && !official::cache_read_known(&price.provider, &price.pricing_id))
            || (tokens.cache_write_1h > 0
                && !matches!(price.provider.as_str(), "anthropic" | "moonshotai"))
            || (tokens.cache_write > 0 && price.cache_write_per_million_usd.is_none())
        {
            return PriceQuote::Unpriced;
        }

        let total = token_cost(tokens.fresh_input, price.input_per_million_usd)
            + token_cost(tokens.cached_input, price.cache_read_per_million_usd)
            + token_cost(tokens.output, price.output_per_million_usd)
            + token_cost(
                tokens.cache_write.saturating_sub(tokens.cache_write_1h),
                price.cache_write_per_million_usd.unwrap_or(Decimal::ZERO),
            )
            + token_cost(
                tokens.cache_write_1h,
                price.input_per_million_usd * Decimal::from(2),
            );
        let total_microusd = total.round().to_i64().unwrap_or(i64::MAX);

        PriceQuote::Priced {
            pricing_id: accounting_id,
            revision: price.revision,
            total_microusd,
        }
    }
}

pub fn canonical_model_provider(raw: &str) -> String {
    let provider = raw.trim();
    if provider.eq_ignore_ascii_case("openai") {
        "openai".to_string()
    } else if provider.is_empty()
        || provider.eq_ignore_ascii_case("unknown")
        || provider.eq_ignore_ascii_case("(unknown)")
    {
        "unknown".to_string()
    } else {
        "custom".to_string()
    }
}

pub fn is_statistics_excluded_model(model: &str) -> bool {
    model.trim().eq_ignore_ascii_case("codex-auto-review")
}

pub fn is_cost_excluded(_provider: &str, model: &str) -> bool {
    is_statistics_excluded_model(model)
}

/// Older migrations use their original, limited alias set.
pub(crate) fn legacy_model_name(raw: &str) -> String {
    let mut name = model_slug(raw);
    loop {
        match name.as_str() {
            "gpt-6.1-sol" | "gpt-6.1-sol-fast" => return "gpt-6.1-sol".into(),
            "deepseek-v4.1-flash" | "deepseek-v4.1-flash-free" => {
                return "deepseek-v4.1-flash".into();
            }
            _ => {}
        }
        let Some(next) = strip_snapshot_suffix(&name) else {
            return raw.to_string();
        };
        name = next;
    }
}

/// Platform labels collapse into the underlying model. Vendor variants such
/// as Flash, FlashX, and Preview keep their own model identities.
pub fn canonical_model_name(raw: &str) -> String {
    let slug = model_slug(raw);
    let slug = strip_effort_label(&slug).unwrap_or(&slug).to_owned();
    let mut name = slug.clone();
    let mut route_base = None;
    loop {
        if name == "gpt-6.1-sol" {
            return name;
        }
        if let Some(id) = official::identity(&name) {
            return if id != name || route_base.is_some() {
                id.to_string()
            } else {
                slug
            };
        }
        if let Some(base) = ["-free", "-fast", ":free", ":fast", "_free", "_fast"]
            .iter()
            .find_map(|suffix| name.strip_suffix(suffix))
            .filter(|base| !base.is_empty())
            .or_else(|| {
                name.strip_suffix("free")
                    .or_else(|| name.strip_suffix("fast"))
                    .filter(|base| official::identity(base).is_some())
            })
        {
            name = strip_effort_label(base).unwrap_or(base).to_owned();
            route_base = Some(name.clone());
        } else if let Some(base) = strip_snapshot_suffix(&name) {
            name = base;
        } else {
            return route_base.unwrap_or(slug);
        }
    }
}

fn model_slug(raw: &str) -> String {
    let name = raw
        .trim()
        .rsplit('/')
        .next()
        .unwrap_or(raw)
        .trim()
        .to_ascii_lowercase();
    if name == "(unknown)" {
        return "unknown".into();
    }
    if let Some((base, effort)) = name.split_once(':')
        && matches!(effort, "minimal" | "low" | "medium" | "high" | "xhigh")
    {
        base.to_string()
    } else {
        name
    }
}

fn strip_snapshot_suffix(value: &str) -> Option<String> {
    strip_iso_date_suffix(value)
        .or_else(|| strip_compact_date_suffix(value))
        .or_else(|| strip_reasoning_suffix(value))
}

pub(crate) fn canonical_pricing_id(raw: &str) -> String {
    raw.split_once('@').map_or_else(
        || canonical_model_name(raw),
        |(base, tier)| {
            format!(
                "{}@{}",
                canonical_model_name(base),
                tier.trim().to_ascii_lowercase()
            )
        },
    )
}

/// 生成从最具体到最宽泛的计价候选，先统一已确认的模型身份。
pub fn normalize_model_id(raw: &str) -> NormalizedModelId {
    let exact = canonical_model_name(raw);

    let mut candidates = vec![exact.clone()];
    let mut index = 0;
    while index < candidates.len() {
        let candidate = candidates[index].clone();
        push_candidate(&mut candidates, strip_iso_date_suffix(&candidate));
        push_candidate(&mut candidates, strip_compact_date_suffix(&candidate));
        push_candidate(&mut candidates, strip_reasoning_suffix(&candidate));
        push_candidate(&mut candidates, historical_tier_alias(&candidate));
        index += 1;
    }

    NormalizedModelId { exact, candidates }
}

/// Resolve a model's vendor using the same identities as the built-in catalog.
pub(crate) fn model_vendor(model: &str) -> &'static str {
    static VENDORS: OnceLock<HashMap<String, String>> = OnceLock::new();
    let vendors = VENDORS.get_or_init(|| {
        builtin_model_prices()
            .into_iter()
            .map(|price| (price.pricing_id, price.provider))
            // Identity is known even when the vendor has no standard hosted rate.
            .chain([
                ("gpt-reserve".into(), "openai".into()),
                ("gpt-oss-120b".into(), "openai".into()),
                ("gpt-oss-20b".into(), "openai".into()),
                ("nemotron-3.5-super-vl-120b-a12b".into(), "nvidia".into()),
                ("nemotron-3-ultra-550b-a55b".into(), "nvidia".into()),
            ])
            .collect()
    });
    vendors.get(model).map(String::as_str).unwrap_or_else(|| {
        normalize_model_id(model)
            .candidates
            .iter()
            .find_map(|candidate| vendors.get(candidate).map(String::as_str))
            .unwrap_or("unknown")
    })
}

/// 为已经下架且官方价格表不再单列的 Codex 型号提供可审计的同层级回退。
/// `gpt-5.2-codex` 的原始 ID 仍保留在统计中，计价 ID 明确显示为 `gpt-5.2`。
fn historical_tier_alias(value: &str) -> Option<String> {
    match value {
        "gpt-5.2-codex" => Some("gpt-5.2".to_string()),
        _ => None,
    }
}

/// 生成用于数据库名称排序的模型强度键。版本优先于同版本的强度层级，
/// 因此降序稳定得到 6.1 Sol、6.1 Terra、6 Astra、6 Sol、6 Luna、5.6 Sol、5.6 Terra、
/// 5.6 Luna、5.5…。
pub fn model_strength_sort_key(raw: &str) -> i64 {
    let normalized = normalize_model_id(raw).exact;
    let (family, rest) = if let Some(rest) = normalized.strip_prefix("gpt-") {
        (4_i64, rest)
    } else if let Some(rest) = normalized.strip_prefix('o') {
        (1_i64, rest)
    } else {
        return 0;
    };
    let version = rest.split('-').next().unwrap_or_default();
    let mut parts = version.split('.');
    let Some(major) = parts.next().and_then(|part| part.parse::<i64>().ok()) else {
        return 0;
    };
    let minor = parts
        .next()
        .and_then(|part| part.parse::<i64>().ok())
        .unwrap_or(0);
    let tier = if rest.contains("-astra") {
        1_000
    } else if rest.contains("-pro") {
        900
    } else if rest.contains("-sol") {
        800
    } else if rest.contains("-terra") {
        700
    } else if rest.contains("-luna") {
        500
    } else if rest.contains("-mini") {
        300
    } else if rest.contains("-nano") {
        200
    } else if rest.contains("-spark") {
        100
    } else {
        600
    };
    family.saturating_mul(1_000_000_000).saturating_add(
        major
            .saturating_mul(1_000_000)
            .saturating_add(minor.saturating_mul(10_000))
            .saturating_add(tier),
    )
}

fn token_cost(tokens: u64, price_per_million_usd: Decimal) -> Decimal {
    // 每 Token 的 microUSD 数值恰好等于“每百万 Token 的 USD 价格”。
    Decimal::from(tokens) * price_per_million_usd
}

fn push_candidate(candidates: &mut Vec<String>, candidate: Option<String>) {
    if let Some(candidate) = candidate
        && !candidate.is_empty()
        && !candidates.contains(&candidate)
    {
        candidates.push(candidate);
    }
}

fn strip_iso_date_suffix(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    if bytes.len() < 11 {
        return None;
    }
    let suffix = &bytes[bytes.len() - 11..];
    let valid = suffix[0] == b'-'
        && suffix[1..5].iter().all(u8::is_ascii_digit)
        && suffix[5] == b'-'
        && suffix[6..8].iter().all(u8::is_ascii_digit)
        && suffix[8] == b'-'
        && suffix[9..11].iter().all(u8::is_ascii_digit);
    valid.then(|| value[..value.len() - 11].to_string())
}

fn strip_compact_date_suffix(value: &str) -> Option<String> {
    let (prefix, suffix) = value.rsplit_once('-')?;
    (suffix.len() == 8 && suffix.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| prefix.to_string())
}

fn strip_reasoning_suffix(value: &str) -> Option<String> {
    let (prefix, suffix) = value.rsplit_once('-')?;
    matches!(suffix, "minimal" | "low" | "medium" | "high" | "xhigh").then(|| prefix.to_string())
}

fn strip_effort_label(value: &str) -> Option<&str> {
    // Some names contain a model tier (e.g. Qwen4-Max or GPT-5.1-Codex-Max).
    // Reserve the ambiguous bare "-max"/"-ultra" spellings for supported families.
    let (base, effort, explicit) =
        if let Some((base, effort)) = value.strip_suffix(')').and_then(|v| v.rsplit_once('(')) {
            (base.trim_end(), effort.trim(), true)
        } else if let Some(base) = value.strip_suffix("-extra-high") {
            return (!base.is_empty()).then_some(base);
        } else {
            let pos = value.rfind(['-', ':', '_', ' '])?;
            (
                &value[..pos],
                &value[pos + 1..],
                value.as_bytes()[pos] != b'-',
            )
        };
    let known = match effort {
        "minimal" | "low" | "medium" | "high" | "xhigh" | "extra" | "extra-high" | "extra high" => {
            true
        }
        "thinking" => base.starts_with("claude-"),
        "max" | "ultra" | "none" => {
            explicit
                || base.starts_with("claude-")
                || (base.starts_with("gpt-") && !base.ends_with("-codex"))
                || base.starts_with("gemini-4-")
        }
        _ => false,
    };
    (known && !base.is_empty()).then_some(base)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    fn price(id: &str) -> ModelPrice {
        ModelPrice {
            provider: "openai".to_string(),
            pricing_id: id.to_string(),
            input_per_million_usd: Decimal::from_str("2.50").unwrap(),
            output_per_million_usd: Decimal::from_str("10.00").unwrap(),
            cache_read_per_million_usd: Decimal::from_str("0.25").unwrap(),
            cache_write_per_million_usd: None,
            revision: 3,
        }
    }

    #[test]
    fn preserves_dynamic_gpt_5_6_variants() {
        for model in ["gpt-5.6", "gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna"] {
            assert_eq!(normalize_model_id(model).exact, model);
        }
    }

    #[test]
    fn preserves_dynamic_gpt_6_variants() {
        for model in [
            "gpt-6.1-sol",
            "gpt-6.1-terra",
            "gpt-6-astra",
            "gpt-6-sol",
            "gpt-6-luna",
        ] {
            assert_eq!(normalize_model_id(model).exact, model);
        }
    }

    #[test]
    fn sorts_models_by_version_then_strength_tier() {
        let ordered = [
            "gpt-6.1-sol",
            "gpt-6.1-terra",
            "gpt-6-astra",
            "gpt-6-sol",
            "gpt-6-luna",
            "gpt-5.6-sol",
            "gpt-5.6-terra",
            "gpt-5.6-luna",
            "gpt-5.5",
            "o4",
            "o4-mini",
            "o3",
        ];
        assert!(
            ordered.windows(2).all(|pair| {
                model_strength_sort_key(pair[0]) > model_strength_sort_key(pair[1])
            })
        );
    }

    #[test]
    fn canonicalizes_visible_providers_without_promoting_unknown_values() {
        assert_eq!(canonical_model_provider(" OpenAI "), "openai");
        assert_eq!(canonical_model_provider("codex_local_access"), "custom");
        assert_eq!(canonical_model_provider("azure"), "custom");
        assert_eq!(canonical_model_provider("unknown"), "unknown");
    }

    #[test]
    fn prices_current_codex_exactly_and_retired_codex_by_visible_tier_alias() {
        let catalog = PricingCatalog::new(builtin_model_prices());
        let tokens = PriceableTokens {
            cache_write_1h: 0,
            fresh_input: 1_000_000,
            cached_input: 0,
            output: 0,
            cache_write: 0,
        };
        assert!(matches!(
            catalog.quote("openai", "gpt-5.3-codex", tokens),
            PriceQuote::Priced { pricing_id, total_microusd: 1_750_000, .. }
                if pricing_id == "gpt-5.3-codex"
        ));
        assert!(matches!(
            catalog.quote("openai", "gpt-5.2-codex", tokens),
            PriceQuote::Priced { pricing_id, total_microusd: 1_750_000, .. }
                if pricing_id == "gpt-5.2"
        ));
    }

    #[test]
    fn prices_gpt_6_astra_at_the_official_standard_short_context_rate() {
        let catalog = PricingCatalog::new(builtin_model_prices());
        let tokens = PriceableTokens {
            cache_write_1h: 0,
            fresh_input: 1_000_000,
            cached_input: 1_000_000,
            output: 1_000_000,
            cache_write: 1_000_000,
        };
        assert!(matches!(
            catalog.quote("openai", "gpt-6-astra", tokens),
            PriceQuote::Priced { pricing_id, total_microusd: 73_500_000, .. }
                if pricing_id == "gpt-6-astra"
        ));
    }

    #[test]
    fn prices_gpt_6_sol_and_luna_at_the_official_standard_short_context_rates() {
        let catalog = PricingCatalog::new(builtin_model_prices());
        let tokens = PriceableTokens {
            cache_write_1h: 0,
            fresh_input: 1_000_000,
            cached_input: 1_000_000,
            output: 1_000_000,
            cache_write: 1_000_000,
        };
        assert!(matches!(
            catalog.quote("openai", "gpt-6-sol", tokens),
            PriceQuote::Priced { pricing_id, total_microusd: 14_700_000, .. }
                if pricing_id == "gpt-6-sol"
        ));
        assert!(matches!(
            catalog.quote("openai", "gpt-6-luna", tokens),
            PriceQuote::Priced { pricing_id, total_microusd: 735_000, .. }
                if pricing_id == "gpt-6-luna"
        ));
    }

    #[test]
    fn prices_gpt_6_1_models_and_updated_gpt_5_6_rates() {
        let catalog = PricingCatalog::new(builtin_model_prices());
        let tokens = PriceableTokens {
            cache_write_1h: 0,
            fresh_input: 1_000_000,
            cached_input: 1_000_000,
            output: 1_000_000,
            cache_write: 1_000_000,
        };
        for (model, expected_cost) in [
            ("gpt-6.1-sol", 14_600_000),
            ("gpt-6.1-terra", 7_300_000),
            ("gpt-5.6-sol", 29_400_000),
            ("gpt-5.6-terra", 16_700_000),
            ("gpt-5.6-luna", 1_670_000),
        ] {
            assert_eq!(
                catalog.quote("openai", model, tokens),
                PriceQuote::Priced {
                    pricing_id: model.to_string(),
                    revision: BUILTIN_PRICING_REVISION,
                    total_microusd: expected_cost,
                }
            );
        }
        assert_eq!(
            catalog.quote("custom", "OpenAI/GPT-6.1-SOL-2026-09-29-high", tokens),
            catalog.quote("openai", "gpt-6.1-sol", tokens)
        );
        assert_eq!(
            catalog.quote("openai", "gpt-6.1-sol-unknown", tokens),
            PriceQuote::Unpriced
        );
        assert_eq!(
            catalog.quote("custom", "OpenAI/GPT-6.1-TERRA-2026-10-26-high", tokens),
            catalog.quote("openai", "gpt-6.1-terra", tokens)
        );
        assert_eq!(
            catalog.quote("openai", "gpt-6.1-terra-unknown", tokens),
            PriceQuote::Unpriced
        );
    }

    #[test]
    fn adds_provider_date_and_reasoning_candidates() {
        let normalized = normalize_model_id("OpenAI/GPT-5.6-2026-07-10-high");
        assert_eq!(normalized.exact, "gpt-5.6-2026-07-10");
        assert!(
            normalized
                .candidates
                .contains(&"gpt-5.6-2026-07-10".to_string())
        );

        let dated = normalize_model_id("openai/gpt-5.6-2026-07-10");
        assert!(dated.candidates.contains(&"gpt-5.6".to_string()));

        let effort = normalize_model_id("openai/gpt-5.6:high");
        assert_eq!(effort.exact, "gpt-5.6");
    }

    #[test]
    fn matches_exact_before_family_prefix_and_computes_micro_usd() {
        let catalog = PricingCatalog::new(vec![price("gpt-5.6"), price("gpt-5.6-sol")]);
        let quote = catalog.quote(
            "OpenAI",
            "openai/GPT-5.6-SOL",
            PriceableTokens {
                cache_write_1h: 0,
                fresh_input: 1000,
                cached_input: 4000,
                output: 500,
                cache_write: 0,
            },
        );
        assert_eq!(
            quote,
            PriceQuote::Priced {
                pricing_id: "gpt-5.6-sol".to_string(),
                revision: 3,
                total_microusd: 8_500,
            }
        );
    }

    #[test]
    fn reports_unpriced_instead_of_zero() {
        let catalog = PricingCatalog::new(vec![price("gpt-5.6")]);
        assert_eq!(
            catalog.quote(
                "openai",
                "unknown-future-model",
                PriceableTokens {
                    cache_write_1h: 0,
                    fresh_input: 1,
                    cached_input: 0,
                    output: 0,
                    cache_write: 0,
                }
            ),
            PriceQuote::Unpriced
        );
    }

    #[test]
    fn excludes_internal_auto_review_without_creating_a_zero_price() {
        let catalog = PricingCatalog::new(vec![price("gpt-5")]);
        assert_eq!(
            catalog.quote(
                "openai",
                "codex-auto-review",
                PriceableTokens {
                    cache_write_1h: 0,
                    fresh_input: 10,
                    cached_input: 20,
                    output: 3,
                    cache_write: 0
                },
            ),
            PriceQuote::Excluded,
        );
    }

    #[test]
    fn does_not_guess_prices_for_unknown_family_suffixes() {
        let catalog = PricingCatalog::new(vec![price("gpt-5")]);
        assert_eq!(
            catalog.quote(
                "openai",
                "gpt-5.3-codex",
                PriceableTokens {
                    cache_write_1h: 0,
                    fresh_input: 10,
                    cached_input: 0,
                    output: 1,
                    cache_write: 0
                },
            ),
            PriceQuote::Unpriced,
        );
    }

    #[test]
    fn custom_provider_falls_back_only_to_an_exact_openai_model_price() {
        let catalog = PricingCatalog::new(vec![price("gpt-5")]);
        assert!(matches!(
            catalog.quote(
                "custom",
                "gpt-5",
                PriceableTokens { fresh_input: 10, cached_input: 0, output: 1, cache_write: 0, cache_write_1h: 0 },
            ),
            PriceQuote::Priced { pricing_id, .. } if pricing_id == "gpt-5"
        ));
        assert_eq!(
            catalog.quote(
                "custom",
                "gpt-5.3-codex",
                PriceableTokens {
                    cache_write_1h: 0,
                    fresh_input: 10,
                    cached_input: 0,
                    output: 1,
                    cache_write: 0
                },
            ),
            PriceQuote::Unpriced,
        );
    }

    fn usage(input: u64) -> PriceableTokens {
        PriceableTokens {
            fresh_input: input,
            cached_input: 0,
            output: 0,
            cache_write: 0,
            cache_write_1h: 0,
        }
    }

    fn at(iso: &str) -> i64 {
        chrono::DateTime::parse_from_rfc3339(iso)
            .unwrap()
            .timestamp_millis()
    }

    #[test]
    fn platform_labels_share_model_identity_and_official_rates() {
        let catalog = PricingCatalog::new(builtin_model_prices());
        for (alias, model, expected) in [
            ("google/gemini-4-neon-xhigh", "gemini-4-neon", 3_000_000),
            ("gemini-4-neon (medium)", "gemini-4-neon", 3_000_000),
            ("claude-opus-5-5-high", "claude-opus-5-5", 4_000_000),
            ("OpenAI/GPT-6.1-Sol-Fast-high", "gpt-6.1-sol", 2_000_000),
            ("gpt-6-astra-fast", "gpt-6-astra", 10_000_000),
            ("hy4free", "hy4", 834_000),
            ("hy4-preview-f", "hy4-preview", 834_000),
            ("kimi-k3-1", "kimi-k3.1-pro", 2_000_000),
            ("glm5.3", "glm-5.3", 1_400_000),
            ("glm5.3flashfree", "glm-5.3-flash", 150_000),
            ("glm-5.3-flash-free", "glm-5.3-flash", 150_000),
            ("mimo-v2.6-flash-free", "mimo-v2.6-flash", 140_000),
        ] {
            assert_eq!(canonical_model_name(alias), model);
            for provider in ["workbuddy", "opencode", "custom", "unknown"] {
                assert!(
                    matches!(
                        catalog.quote(provider, alias, usage(1_000_000)),
                        PriceQuote::Priced { pricing_id, total_microusd, .. }
                            if pricing_id == model && total_microusd == expected
                    ),
                    "{provider}/{alias}"
                );
            }
        }
        for model in ["qwen4-max", "gpt-5.1-codex-max", "qwen3-thinking"] {
            assert_eq!(canonical_model_name(model), model);
        }
        assert_eq!(model_vendor("gpt-reserve"), "openai");
        assert_eq!(model_vendor("gemini-4-neon-medium"), "google");
        for provider in ["opencode", "deepseek-official", "workbuddy"] {
            for (time, expected) in [
                ("2026-10-27T02:00:00Z", 300_000),
                ("2026-10-27T12:00:00Z", 150_000),
            ] {
                assert!(matches!(
                    catalog.quote_at(provider, "deepseek-v4.1-flash-free", usage(1_000_000), at(time)),
                    PriceQuote::Priced { total_microusd, .. } if total_microusd == expected
                ));
            }
        }
        assert_eq!(
            catalog.quote("opencode", "nemotron-3.5-super-vl-free", usage(1_000_000)),
            PriceQuote::Unpriced
        );
        assert_eq!(
            canonical_model_name("nemotron-3.5-super-vl-free"),
            "nemotron-3.5-super-vl-120b-a12b"
        );
        assert!(matches!(
            catalog.quote("opencode", "qwen3.8-max", usage(1_000_000)),
            PriceQuote::Priced {
                total_microusd: 2_000_000,
                ..
            }
        ));
        assert_eq!(
            catalog.quote(
                "opencode",
                "qwen3.8-max",
                PriceableTokens {
                    cached_input: 1,
                    ..usage(1_000_000)
                },
            ),
            PriceQuote::Unpriced
        );
    }

    #[test]
    fn deepseek_peak_windows_weekends_and_holidays_use_native_request_time() {
        let catalog = PricingCatalog::new(builtin_model_prices());
        for (time, peak) in [
            ("2026-10-26T00:59:59Z", false),
            ("2026-10-26T01:00:00Z", true),
            ("2026-10-26T03:59:59Z", true),
            ("2026-10-26T04:00:00Z", false),
            ("2026-10-26T06:00:00Z", true),
            ("2026-10-26T10:00:00Z", false),
            ("2026-10-24T02:00:00Z", false),
            ("2026-10-01T02:00:00Z", false),
        ] {
            assert!(
                matches!(catalog.quote_at("deepseek-official","deepseek-v4-pro",usage(1_000_000),at(time)),PriceQuote::Priced{total_microusd,..} if total_microusd == if peak {1_320_000} else {660_000}),
                "{time}"
            );
        }
        assert_eq!(
            catalog.quote_summary(
                "deepseek-official",
                "deepseek-v4-pro",
                usage(2_000_000),
                "deepseek-v4-pro@offpeak"
            ),
            PriceQuote::Priced {
                pricing_id: "deepseek-v4-pro@offpeak".into(),
                revision: BUILTIN_PRICING_REVISION,
                total_microusd: 1_320_000
            }
        );
    }

    #[test]
    fn context_brackets_and_announced_price_changes_are_preserved_in_summaries() {
        let catalog = PricingCatalog::new(builtin_model_prices());
        for (provider, id, threshold, lower, upper) in [
            ("openai", "gpt-6.2-astra", 272001, 8.0, 16.0),
            ("openai", "gpt-6.2-sol", 272001, 2.0, 4.0),
            ("google", "gemini-2.5-pro", 200001, 1.25, 2.5),
            ("minimax", "minimax-m3", 512001, 0.3, 0.6),
            ("alibaba", "qwen3.7-plus", 256001, 0.4, 1.2),
            ("anthropic", "claude-haiku-5-5", 100001, 0.1, 0.5),
            ("xai", "grok-4.7", 200000, 2.0, 4.0),
        ] {
            assert!(
                matches!(catalog.quote(provider,id,usage(threshold-1)),PriceQuote::Priced{pricing_id,..} if pricing_id==id)
            );
            let quote = catalog.quote(provider, id, usage(threshold));
            assert!(
                matches!(quote,PriceQuote::Priced{pricing_id,..} if pricing_id==format!("{id}@long"))
            );
            assert!(
                matches!(catalog.quote_summary(provider,id,usage(1_000_000),id),PriceQuote::Priced{total_microusd,..} if total_microusd==(1_000_000.0*lower) as i64)
            );
            assert!(
                matches!(catalog.quote_summary(provider,id,usage(1_000_000),&format!("{id}@long")),PriceQuote::Priced{total_microusd,..} if total_microusd==(1_000_000.0*upper) as i64)
            );
        }
        assert!(matches!(
            catalog.quote_at(
                "google",
                "gemini-4-argon",
                usage(1_000_000),
                at("2026-11-30T23:59:59Z")
            ),
            PriceQuote::Priced {
                total_microusd: 2_000_000,
                ..
            }
        ));
        assert!(matches!(
            catalog.quote_at(
                "google",
                "gemini-4-argon",
                usage(1_000_000),
                at("2026-12-01T00:00:00Z")
            ),
            PriceQuote::Priced {
                total_microusd: 4_000_000,
                ..
            }
        ));
    }

    #[test]
    fn newly_announced_price_cutovers_use_request_time() {
        let catalog = PricingCatalog::new(builtin_model_prices());
        for (provider, model, before, after, initial, regular) in [
            (
                "google",
                "gemini-4-carbon",
                "2026-11-30T23:59:59Z",
                "2026-12-01T00:00:00Z",
                2_000_000,
                4_000_000,
            ),
            (
                "tencent",
                "hy4-flash",
                "2026-11-30T03:59:59Z",
                "2026-11-30T04:00:00Z",
                35_000,
                70_000,
            ),
        ] {
            for (time, id, cost) in [
                (before, model.to_string(), initial),
                (after, format!("{model}@regular"), regular),
            ] {
                assert_eq!(
                    catalog.quote_at(provider, model, usage(1_000_000), at(time)),
                    PriceQuote::Priced {
                        pricing_id: id,
                        revision: BUILTIN_PRICING_REVISION,
                        total_microusd: cost,
                    }
                );
            }
        }
    }

    #[test]
    fn decisions_without_a_cache_rate_price_only_observed_uncached_input() {
        let catalog = PricingCatalog::new(builtin_model_prices());
        assert!(matches!(
            catalog.quote("google", "gemini-decision-1", usage(1_000_000)),
            PriceQuote::Priced {
                total_microusd: 100_000,
                ..
            }
        ));
        assert_eq!(
            catalog.quote(
                "google",
                "gemini-decision-1",
                PriceableTokens {
                    cached_input: 1,
                    ..usage(1_000_000)
                }
            ),
            PriceQuote::Unpriced
        );
    }

    #[test]
    fn official_catalog_has_unique_keys_and_traceable_sources() {
        let prices = builtin_model_prices();
        let keys: HashSet<_> = prices.iter().map(|p| &p.pricing_id).collect();
        assert_eq!(keys.len(), prices.len());
        for price in prices {
            assert!(
                official::price_source(&price.provider, &price.pricing_id).starts_with("https://")
            );
        }
    }
}
