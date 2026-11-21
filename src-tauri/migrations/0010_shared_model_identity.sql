-- Keep user edits when route prices converge. The newest explicit edit wins.
DELETE FROM model_prices
WHERE provider = 'opencode' AND is_builtin = 1 AND is_overridden = 0;

CREATE TEMP TABLE canonical_price_rows AS
SELECT id, current_pricing_id(pricing_id) AS pricing_id,
       ROW_NUMBER() OVER (
           PARTITION BY current_pricing_id(pricing_id)
           ORDER BY (is_builtin = 0 OR is_overridden = 1 OR is_deleted = 1) DESC,
                    updated_at_ms DESC, id DESC
       ) AS preference
FROM model_prices;

DELETE FROM model_prices WHERE id IN (SELECT id FROM canonical_price_rows WHERE preference > 1);
UPDATE model_prices SET
    display_name = CASE WHEN LOWER(display_name) = pricing_id THEN current_pricing_id(pricing_id) ELSE display_name END,
    pricing_id = (SELECT r.pricing_id FROM canonical_price_rows r WHERE r.id = model_prices.id),
    source_url = CASE WHEN provider = 'opencode' THEN NULL ELSE source_url END,
    source_updated_at_ms = CASE WHEN provider = 'opencode' THEN NULL ELSE source_updated_at_ms END,
    is_builtin = CASE WHEN provider = 'opencode' THEN 0 ELSE is_builtin END,
    is_overridden = CASE WHEN provider = 'opencode' THEN 0 ELSE is_overridden END;
DROP TABLE canonical_price_rows;

CREATE UNIQUE INDEX idx_model_prices_by_id ON model_prices(pricing_id);
