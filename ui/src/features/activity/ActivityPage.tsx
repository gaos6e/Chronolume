import { keepPreviousData, useQuery } from '@tanstack/react-query';
import { useState } from 'react';
import { Binary, Boxes, TerminalSquare, Wrench } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { getTools } from '../../api';
import { QueryStatus } from '../../components/QueryStatus';
import type { UsageFilters } from '../../types';

export function ActivityPage({ filters }: { filters: UsageFilters }) {
  const { t } = useTranslation();
  const [tableOpen, setTableOpen] = useState(false);
  const tools = useQuery({ queryKey: ['tools', filters], queryFn: () => getTools(filters), placeholderData: keepPreviousData });
  const toolTrendMax = Math.max(1, ...(tools.data?.trend.map((item) => item.callCount) ?? []));
  const categoryMax = Math.max(1, ...(tools.data?.categories.map((item) => item.callCount) ?? []));
  return <section className="feature-page">
    <QueryStatus
      loading={tools.isLoading && !tools.data}
      error={tools.error}
      refreshing={tools.isFetching && Boolean(tools.data)}
      onRetry={() => { void tools.refetch(); }}
    />
    {tools.data && <>
    <div className="stat-card-grid">
      <div className="stat-card"><Wrench /><span>{t('工具调用')}</span><strong>{tools.data?.totalCalls.toLocaleString() ?? '—'}</strong></div>
      <div className="stat-card"><Boxes /><span>{t('不同工具')}</span><strong>{tools.data?.uniqueTools.toLocaleString() ?? '—'}</strong></div>
      <div className="stat-card"><Binary /><span>{t('结构化类别')}</span><strong>{tools.data?.categories.length ?? '—'}</strong></div>
      <div className="stat-card"><TerminalSquare /><span>{t('原始命令')}</span><strong>{t('不落库')}</strong></div>
    </div>
    <section className="feature-card"><div className="feature-card-heading"><div><span>TREND</span><h2>{t('每日工具调用')}</h2></div></div>
      {tools.data.trend.length === 0 ? <div className="chart-empty">{t('当前范围内没有工具活动。')}</div> : <div className="spark-bars" role="img" aria-label={t('每日工具调用')}>{tools.data?.trend.map((point) => {
        return <i key={point.date} style={{ height: `${Math.max(2, (point.callCount / toolTrendMax) * 100)}%` }} title={`${point.date} · ${point.callCount.toLocaleString()}`} />;
      })}</div>}
      {tools.data.trend.length > 0 && <details className="chart-data" onToggle={(event) => setTableOpen(event.currentTarget.open)}>
        <summary>{t('查看图表数据')}</summary>
        {tableOpen && <div className="data-table-wrap" tabIndex={0} role="region" aria-label={t('每日工具调用')}>
          <table className="data-table">
            <thead><tr><th>{t('日期')}</th><th>{t('工具调用')}</th></tr></thead>
            <tbody>{tools.data.trend.map((point) => <tr key={point.date}><td>{point.date}</td><td>{point.callCount.toLocaleString()}</td></tr>)}</tbody>
          </table>
        </div>}
      </details>}
    </section>
    <div className="two-column-grid">
      <section className="feature-card"><div className="feature-card-heading"><div><span>CATEGORIES</span><h2>{t('读写与执行')}</h2></div></div>
        <div className="bar-list">{tools.data?.categories.map((category) => {
          return <div key={category.category}><span>{t(category.category)}</span><div aria-hidden="true"><i style={{ width: `${(category.callCount / categoryMax) * 100}%` }} /></div><strong>{category.callCount.toLocaleString()}</strong></div>;
        })}</div>
        {tools.data.categories.length === 0 && <div className="table-empty">{t('当前范围内没有工具活动。')}</div>}
      </section>
      <section className="feature-card"><div className="feature-card-heading"><div><span>TOOLS</span><h2>{t('Top 工具')}</h2></div></div>
        <div className="compact-list tool-list-scroll" tabIndex={tools.data.topTools.length > 0 ? 0 : undefined} role="region" aria-label={t('Top 工具')}>{tools.data?.topTools.map((tool) => <div key={`${tool.toolName}-${tool.category}-${tool.operationKind}`}><span>{tool.toolName}<small>{t(tool.category)} · {t(tool.operationKind)}</small></span><strong>{tool.callCount.toLocaleString()}</strong></div>)}</div>
        {!tools.isLoading && tools.data?.totalCalls === 0 && <div className="table-empty">{t('当前范围内没有工具活动。')}</div>}
      </section>
    </div>
    </>}
  </section>;
}
