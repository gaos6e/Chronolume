import { Search, X } from 'lucide-react';
import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { WorkspaceCatalogItem } from '../types';
import { Dialog } from './Dialog';
import { ActionFeedback } from './ActionFeedback';
import { QueryStatus } from './QueryStatus';

interface WorkspaceVisibilityDialogProps {
  options: WorkspaceCatalogItem[];
  selectedIds: string[];
  saving: boolean;
  loading?: boolean;
  error?: unknown;
  saveError?: unknown;
  onRetry?: () => void;
  onClose: () => void;
  onSave: (ids: string[]) => void;
}

export function WorkspaceVisibilityDialog({
  options,
  selectedIds,
  saving,
  loading = false,
  error,
  saveError,
  onRetry = () => undefined,
  onClose,
  onSave,
}: WorkspaceVisibilityDialogProps) {
  const { t } = useTranslation();
  const [search, setSearch] = useState('');
  const [draft, setDraft] = useState(() => new Set(selectedIds));
  const visibleOptions = useMemo(() => {
    const needle = search.trim().toLocaleLowerCase();
    return needle
      ? options.filter((option) => `${option.label}\n${option.normalizedPath}`.toLocaleLowerCase().includes(needle))
      : options;
  }, [options, search]);

  const toggle = (id: string, checked: boolean) => {
    setDraft((current) => {
      const next = new Set(current);
      if (checked) next.add(id);
      else next.delete(id);
      return next;
    });
  };

  return (
    <Dialog className="workspace-dialog" labelledBy="workspace-dialog-title" busy={saving} onClose={onClose}>
        <header>
          <div>
            <h2 id="workspace-dialog-title">{t('选择显示的工作区')}</h2>
            <p>{t('勾选后会显示在项目页和全局工作区快捷列表中。')}</p>
          </div>
          <button type="button" className="icon-button" aria-label={t('关闭')} disabled={saving} onClick={onClose}><X /></button>
        </header>
        <label className="workspace-search">
          <Search aria-hidden="true" />
          <span className="sr-only">{t('搜索')}</span>
          <input value={search} disabled={saving || loading || Boolean(error)} placeholder={t('搜索工作区…')} onChange={(event) => setSearch(event.target.value)} autoFocus data-autofocus />
        </label>
        <div className="workspace-dialog-actions">
          <span>{t('已选择 {{count}} 个', { count: draft.size })}</span>
          <button type="button" disabled={saving || loading || Boolean(error)} onClick={() => setDraft(new Set(options.map((option) => option.value)))}>{t('全选')}</button>
          <button type="button" disabled={saving || loading || Boolean(error)} onClick={() => setDraft(new Set())}>{t('全部取消')}</button>
        </div>
        <QueryStatus loading={loading} error={error} onRetry={onRetry} />
        <ActionFeedback pending={saving} error={saveError} />
        <div className="workspace-option-list">
          {visibleOptions.map((option) => (
            <label key={option.value}>
              <input type="checkbox" disabled={saving} checked={draft.has(option.value)} onChange={(event) => toggle(option.value, event.target.checked)} />
              <span><strong>{option.label}</strong><small>{option.normalizedPath}</small></span>
            </label>
          ))}
          {!loading && !error && visibleOptions.length === 0 && <p>{t(options.length === 0 ? '暂无工作区。请先在桌面应用中同步 Codex 数据。' : '没有匹配的工作区。')}</p>}
        </div>
        <footer>
          <button type="button" className="quiet-button" disabled={saving} onClick={onClose}>{t('取消')}</button>
          <button type="button" className="primary-button" disabled={saving || loading || Boolean(error)} onClick={() => onSave([...draft])}>{t('保存显示范围')}</button>
        </footer>
    </Dialog>
  );
}
