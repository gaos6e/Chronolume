-- Canonicalize model identity without changing event ownership, tokens, timing or costs.

CREATE TEMP TABLE canonical_affected_sessions AS SELECT DISTINCT session_id FROM session_model_segments WHERE model_raw <> canonical_model_name(model_raw) OR pricing_model_id <> canonical_model_name(pricing_model_id);

CREATE TEMP TABLE canonical_daily_usage AS SELECT session_id, local_date, timezone_id, day_start_utc_ms, day_end_utc_ms, workspace_id, model_provider, canonical_model_name(model_raw) AS model_raw, canonical_model_name(pricing_model_id) AS pricing_model_id, archived, active_ms, input_tokens, fresh_input_tokens, cached_input_tokens, output_tokens, reasoning_tokens, total_tokens, priced_cost_microusd, priced_event_count, unpriced_event_count, last_activity_at_ms, cache_write_tokens, cache_write_1h_tokens, reported_cost_microusd FROM session_daily_usage WHERE model_raw <> canonical_model_name(model_raw) OR pricing_model_id <> canonical_model_name(pricing_model_id);

DELETE FROM session_daily_usage WHERE model_raw <> canonical_model_name(model_raw) OR pricing_model_id <> canonical_model_name(pricing_model_id);

INSERT INTO session_daily_usage (session_id, local_date, timezone_id, day_start_utc_ms, day_end_utc_ms, workspace_id, model_provider, model_raw, pricing_model_id, archived, active_ms, input_tokens, fresh_input_tokens, cached_input_tokens, output_tokens, reasoning_tokens, total_tokens, priced_cost_microusd, priced_event_count, unpriced_event_count, last_activity_at_ms, cache_write_tokens, cache_write_1h_tokens, reported_cost_microusd)

SELECT session_id, local_date, timezone_id, MIN(day_start_utc_ms), MAX(day_end_utc_ms), workspace_id, model_provider, model_raw, pricing_model_id, archived, SUM(active_ms), SUM(input_tokens), SUM(fresh_input_tokens), SUM(cached_input_tokens), SUM(output_tokens), SUM(reasoning_tokens), SUM(total_tokens), SUM(priced_cost_microusd), SUM(priced_event_count), SUM(unpriced_event_count), MAX(last_activity_at_ms), SUM(cache_write_tokens), SUM(cache_write_1h_tokens), SUM(reported_cost_microusd) FROM canonical_daily_usage GROUP BY session_id, local_date, timezone_id, model_provider, model_raw, pricing_model_id, archived

ON CONFLICT(session_id, local_date, timezone_id, model_provider, model_raw, pricing_model_id, archived) DO UPDATE SET

    active_ms = COALESCE(session_daily_usage.active_ms, 0) + COALESCE(excluded.active_ms, 0),
    input_tokens = COALESCE(session_daily_usage.input_tokens, 0) + COALESCE(excluded.input_tokens, 0),
    fresh_input_tokens = COALESCE(session_daily_usage.fresh_input_tokens, 0) + COALESCE(excluded.fresh_input_tokens, 0),
    cached_input_tokens = COALESCE(session_daily_usage.cached_input_tokens, 0) + COALESCE(excluded.cached_input_tokens, 0),
    output_tokens = COALESCE(session_daily_usage.output_tokens, 0) + COALESCE(excluded.output_tokens, 0),
    reasoning_tokens = COALESCE(session_daily_usage.reasoning_tokens, 0) + COALESCE(excluded.reasoning_tokens, 0),
    total_tokens = COALESCE(session_daily_usage.total_tokens, 0) + COALESCE(excluded.total_tokens, 0),
    priced_cost_microusd = COALESCE(session_daily_usage.priced_cost_microusd, 0) + COALESCE(excluded.priced_cost_microusd, 0),
    priced_event_count = COALESCE(session_daily_usage.priced_event_count, 0) + COALESCE(excluded.priced_event_count, 0),
    unpriced_event_count = COALESCE(session_daily_usage.unpriced_event_count, 0) + COALESCE(excluded.unpriced_event_count, 0),
    cache_write_tokens = COALESCE(session_daily_usage.cache_write_tokens, 0) + COALESCE(excluded.cache_write_tokens, 0),
    cache_write_1h_tokens = COALESCE(session_daily_usage.cache_write_1h_tokens, 0) + COALESCE(excluded.cache_write_1h_tokens, 0),
    reported_cost_microusd = CASE WHEN session_daily_usage.reported_cost_microusd IS NULL AND excluded.reported_cost_microusd IS NULL THEN NULL ELSE COALESCE(session_daily_usage.reported_cost_microusd, 0) + COALESCE(excluded.reported_cost_microusd, 0) END,
    day_start_utc_ms = MIN(session_daily_usage.day_start_utc_ms, excluded.day_start_utc_ms),
    day_end_utc_ms = MAX(session_daily_usage.day_end_utc_ms, excluded.day_end_utc_ms),
    last_activity_at_ms = MAX(COALESCE(session_daily_usage.last_activity_at_ms, 0), COALESCE(excluded.last_activity_at_ms, 0));

DROP TABLE canonical_daily_usage;

UPDATE source_files SET current_model_raw = canonical_model_name(current_model_raw), current_pricing_model_id = canonical_model_name(current_pricing_model_id) WHERE current_model_raw <> canonical_model_name(current_model_raw) OR current_pricing_model_id <> canonical_model_name(current_pricing_model_id);

UPDATE sessions SET latest_model_raw = canonical_model_name(latest_model_raw), latest_pricing_model_id = canonical_model_name(latest_pricing_model_id), primary_model_raw = canonical_model_name(primary_model_raw), primary_pricing_model_id = canonical_model_name(primary_pricing_model_id) WHERE latest_model_raw <> canonical_model_name(latest_model_raw) OR latest_pricing_model_id <> canonical_model_name(latest_pricing_model_id) OR primary_model_raw <> canonical_model_name(primary_model_raw) OR primary_pricing_model_id <> canonical_model_name(primary_pricing_model_id);

UPDATE usage_events SET model_raw = canonical_model_name(model_raw), pricing_model_id = canonical_model_name(pricing_model_id) WHERE model_raw <> canonical_model_name(model_raw) OR pricing_model_id <> canonical_model_name(pricing_model_id);

UPDATE session_model_segments SET model_raw = canonical_model_name(model_raw), pricing_model_id = canonical_model_name(pricing_model_id) WHERE model_raw <> canonical_model_name(model_raw) OR pricing_model_id <> canonical_model_name(pricing_model_id);

UPDATE activity_segments SET model_raw = canonical_model_name(model_raw), pricing_model_id = canonical_model_name(pricing_model_id) WHERE model_raw <> canonical_model_name(model_raw) OR pricing_model_id <> canonical_model_name(pricing_model_id);

UPDATE session_turn_latencies SET model_raw = canonical_model_name(model_raw) WHERE model_raw <> canonical_model_name(model_raw);

UPDATE source_files SET first_token_context_json = json_set(first_token_context_json, '$.model_raw', canonical_model_name(json_extract(first_token_context_json, '$.model_raw')))
WHERE json_extract(first_token_context_json, '$.model_raw') <> canonical_model_name(json_extract(first_token_context_json, '$.model_raw'));

UPDATE sessions SET (primary_model_raw, primary_pricing_model_id) = (
    SELECT model_raw, NULLIF(pricing_model_id, '') FROM session_model_segments
    WHERE session_id = sessions.id AND LOWER(TRIM(model_raw)) <> 'codex-auto-review'
    GROUP BY model_provider, model_raw, pricing_model_id
    ORDER BY SUM(input_tokens + output_tokens) DESC, MAX(segment_index) DESC LIMIT 1
) WHERE id IN (SELECT session_id FROM canonical_affected_sessions);

DROP TABLE canonical_affected_sessions;

-- Keep global-only historical rows; use permanent session IDs when available
-- to count sessions once when their model names converge.
CREATE TEMP TABLE canonical_usage_rollups AS SELECT local_date, timezone_id, day_start_utc_ms, day_end_utc_ms, workspace_id, model_provider, canonical_model_name(model_raw) AS model_raw, canonical_model_name(pricing_model_id) AS pricing_model_id, archived, session_count, active_ms, input_tokens, fresh_input_tokens, cached_input_tokens, output_tokens, reasoning_tokens, total_tokens, priced_cost_microusd, unpriced_event_count, last_activity_at_ms, agent, cache_write_tokens, cache_write_1h_tokens, reported_cost_microusd FROM daily_usage_rollups WHERE canonical_model_name(model_raw) = 'gpt-6.1-sol' OR pricing_model_id <> canonical_model_name(pricing_model_id);
DELETE FROM daily_usage_rollups WHERE canonical_model_name(model_raw) = 'gpt-6.1-sol' OR pricing_model_id <> canonical_model_name(pricing_model_id);
INSERT INTO daily_usage_rollups (local_date, timezone_id, day_start_utc_ms, day_end_utc_ms, workspace_id, model_provider, model_raw, pricing_model_id, archived, session_count, active_ms, input_tokens, fresh_input_tokens, cached_input_tokens, output_tokens, reasoning_tokens, total_tokens, priced_cost_microusd, unpriced_event_count, last_activity_at_ms, agent, cache_write_tokens, cache_write_1h_tokens, reported_cost_microusd)
SELECT local_date, timezone_id, MIN(day_start_utc_ms), MAX(day_end_utc_ms), workspace_id, model_provider, model_raw, pricing_model_id, archived, COALESCE(NULLIF((SELECT COUNT(DISTINCT u.session_id) FROM session_daily_usage u WHERE u.agent = r.agent AND u.local_date = r.local_date AND u.timezone_id = r.timezone_id AND u.workspace_id = r.workspace_id AND u.model_provider = r.model_provider AND u.model_raw = r.model_raw AND u.pricing_model_id = r.pricing_model_id AND u.archived = r.archived), 0), SUM(session_count)), SUM(active_ms), SUM(input_tokens), SUM(fresh_input_tokens), SUM(cached_input_tokens), SUM(output_tokens), SUM(reasoning_tokens), SUM(total_tokens), SUM(priced_cost_microusd), SUM(unpriced_event_count), MAX(last_activity_at_ms), agent, SUM(cache_write_tokens), SUM(cache_write_1h_tokens), SUM(reported_cost_microusd) FROM canonical_usage_rollups r GROUP BY agent, local_date, timezone_id, workspace_id, model_provider, model_raw, pricing_model_id, archived;
DROP TABLE canonical_usage_rollups;

DELETE FROM app_settings WHERE key = 'builtin-pricing-revision' AND EXISTS (
    SELECT 1 FROM model_prices WHERE pricing_id <> canonical_model_name(pricing_id)
);

INSERT INTO model_prices (pricing_id, provider, display_name, input_per_million_usd, output_per_million_usd, cache_read_per_million_usd, cache_write_per_million_usd, default_input_per_million_usd, default_output_per_million_usd, default_cache_read_per_million_usd, default_cache_write_per_million_usd, is_builtin, is_overridden, is_deleted, revision, source_url, source_updated_at_ms, created_at_ms, updated_at_ms)

SELECT canonical_model_name(pricing_id), provider, canonical_model_name(display_name), input_per_million_usd, output_per_million_usd, cache_read_per_million_usd, cache_write_per_million_usd, default_input_per_million_usd, default_output_per_million_usd, default_cache_read_per_million_usd, default_cache_write_per_million_usd, is_builtin, is_overridden, is_deleted, revision, source_url, source_updated_at_ms, created_at_ms, updated_at_ms FROM model_prices WHERE pricing_id <> canonical_model_name(pricing_id)

ON CONFLICT(provider, pricing_id) DO UPDATE SET

    input_per_million_usd = CASE WHEN model_prices.is_overridden = 0 AND model_prices.is_builtin = 1 THEN excluded.input_per_million_usd ELSE model_prices.input_per_million_usd END,
    output_per_million_usd = CASE WHEN model_prices.is_overridden = 0 AND model_prices.is_builtin = 1 THEN excluded.output_per_million_usd ELSE model_prices.output_per_million_usd END,
    cache_read_per_million_usd = CASE WHEN model_prices.is_overridden = 0 AND model_prices.is_builtin = 1 THEN excluded.cache_read_per_million_usd ELSE model_prices.cache_read_per_million_usd END,
    cache_write_per_million_usd = CASE WHEN model_prices.is_overridden = 0 AND model_prices.is_builtin = 1 THEN excluded.cache_write_per_million_usd ELSE model_prices.cache_write_per_million_usd END,
    is_deleted = CASE WHEN model_prices.is_overridden = 0 AND model_prices.is_builtin = 1 THEN excluded.is_deleted ELSE model_prices.is_deleted END,
    is_overridden = CASE WHEN model_prices.is_builtin = 1 THEN 1 ELSE model_prices.is_overridden END,
    revision = MAX(model_prices.revision, excluded.revision) + 1;

DELETE FROM model_prices WHERE pricing_id <> canonical_model_name(pricing_id);
