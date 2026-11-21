-- Model usage and retained summaries are merged by merge_model_usage.sql first.
CREATE TEMP TABLE effort_price_rows AS
SELECT id, current_pricing_id(pricing_id) AS base_id,
       ROW_NUMBER() OVER (
           PARTITION BY current_pricing_id(pricing_id)
           ORDER BY (is_builtin = 0 OR is_overridden = 1 OR is_deleted = 1) DESC,
                    updated_at_ms DESC, id DESC
       ) AS preference
FROM model_prices;

DELETE FROM model_prices WHERE id IN (SELECT id FROM effort_price_rows WHERE preference > 1);
UPDATE model_prices SET
    display_name = CASE WHEN LOWER(display_name) = pricing_id THEN current_pricing_id(pricing_id) ELSE display_name END,
    pricing_id = current_pricing_id(pricing_id)
WHERE pricing_id <> current_pricing_id(pricing_id);
DROP TABLE effort_price_rows;
