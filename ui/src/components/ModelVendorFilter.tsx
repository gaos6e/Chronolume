import { useEffect, useId, useRef, useState } from 'react';
import { ChevronDown } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { modelVendorLabel } from '../lib/modelVendors';

interface ModelVendorFilterProps {
  options: string[];
  selected: string[];
  loading?: boolean;
  onChange: (vendors: string[]) => void;
}

export function ModelVendorFilter({ options, selected, loading, onChange }: ModelVendorFilterProps) {
  const { t } = useTranslation();
  const field = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  const labelId = useId();
  const valueId = useId();
  const panelId = useId();
  // Keep a selection clearable even if it has no data in the new date range.
  const available = [...new Set([...options, ...selected])]
    .sort((left, right) => Number(left === 'unknown') - Number(right === 'unknown'));

  useEffect(() => {
    if (!open) return;
    const dismiss = (event: PointerEvent) => {
      if (!field.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener('pointerdown', dismiss);
    return () => document.removeEventListener('pointerdown', dismiss);
  }, [open]);

  const summaryText = selected.length === 0 ? t('全部供应商')
    : selected.length === 1 ? modelVendorLabel(selected[0])
      : t('已选 {{count}} 家供应商', { count: selected.length });

  return <div
    ref={field}
    className="provider-field"
    onBlur={(event) => {
      if (event.relatedTarget instanceof Node && !event.currentTarget.contains(event.relatedTarget)) setOpen(false);
    }}
    onKeyDown={(event) => {
      if (event.key !== 'Escape' || !open) return;
      setOpen(false);
      trigger.current?.focus();
      event.stopPropagation();
    }}
  >
    <span id={labelId}>{t('模型供应商')}</span>
    <div className="provider-filter">
      <button
        ref={trigger}
        type="button"
        className="provider-trigger"
        aria-labelledby={`${labelId} ${valueId}`}
        aria-expanded={open}
        aria-controls={panelId}
        onClick={() => setOpen((value) => !value)}
      >
        <span id={valueId}>{summaryText}</span><ChevronDown aria-hidden="true" />
      </button>
      {open && <div className="provider-panel" id={panelId} aria-busy={loading || undefined}>
        <p className="provider-hint">{t('按模型所属供应商筛选，可多选。')}</p>
        <fieldset className="provider-options" aria-labelledby={labelId}>
          {available.map((vendor) => <label key={vendor}>
            <input
              type="checkbox"
              checked={selected.includes(vendor)}
              onChange={(event) => onChange(event.target.checked
                ? [...selected, vendor].sort()
                : selected.filter((value) => value !== vendor))}
            />
            <span className="provider-checkbox" aria-hidden="true" />
            <span>{modelVendorLabel(vendor)}</span>
          </label>)}
          {available.length === 0 && <p role="status">{t(loading ? '正在读取供应商…' : '当前范围没有可选供应商。')}</p>}
        </fieldset>
        {selected.length > 0 && <button type="button" className="quiet-button" onClick={() => {
          onChange([]);
          trigger.current?.focus();
        }}>{t('清除供应商筛选')}</button>}
      </div>}
    </div>
  </div>;
}
