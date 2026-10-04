import { ChevronLeft, ChevronRight, LoaderCircle, Search } from 'lucide-react';
import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';

interface PageControlsProps {
  search: string;
  sort: string;
  sortOptions: Array<{ value: string; label: string }>;
  descending: boolean;
  page: number;
  pageSize: number;
  total: number;
  loading?: boolean;
  onSearch: (value: string) => void;
  onSort: (value: string) => void;
  onDescending: (value: boolean) => void;
  onPage: (page: number) => void;
}

export function PageControls(props: PageControlsProps) {
  const { t } = useTranslation();
  const pages = Math.max(1, Math.ceil(props.total / props.pageSize));
  useEffect(() => {
    if (!props.loading && props.page >= pages) props.onPage(pages - 1);
  }, [pages, props.loading, props.page, props.onPage]);
  return (
    <div className="page-controls">
      <label className="search-box">
        <Search aria-hidden="true" />
        <span className="sr-only">{t('搜索')}</span>
        <input
          type="search"
          value={props.search}
          placeholder={t('搜索…')}
          onChange={(event) => props.onSearch(event.target.value)}
        />
      </label>
      <label>
        <span className="sr-only">{t('排序')}</span>
        <select value={props.sort} onChange={(event) => props.onSort(event.target.value)}>
          {props.sortOptions.map((option) => (
            <option key={option.value} value={option.value}>{t(option.label)}</option>
          ))}
        </select>
      </label>
      <button
        type="button"
        className="quiet-button"
        onClick={() => props.onDescending(!props.descending)}
      >
        {props.descending ? t('降序') : t('升序')}
      </button>
      <span className="page-count" aria-live="polite"><LoaderCircle aria-hidden="true" className={props.loading ? 'page-loading spin' : 'page-loading'} style={{ visibility: props.loading ? 'visible' : 'hidden' }} />{props.total.toLocaleString()} {t('项')}</span>
      <div className="pager">
        <button type="button" aria-label={t('上一页')} disabled={props.loading || props.page === 0} onClick={() => props.onPage(props.page - 1)}>
          <ChevronLeft />
        </button>
        <span>{props.page + 1} / {pages}</span>
        <button type="button" aria-label={t('下一页')} disabled={props.loading || props.page + 1 >= pages} onClick={() => props.onPage(props.page + 1)}>
          <ChevronRight />
        </button>
      </div>
    </div>
  );
}
