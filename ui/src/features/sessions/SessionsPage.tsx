import { useState } from 'react';
import { keepPreviousData, useQuery } from '@tanstack/react-query';
import { Archive, ChevronRight, Clock3 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { getSessionDetail, getSessions, getUsageEvents } from '../../api';
import { PageControls } from '../../components/PageControls';
import { QueryStatus } from '../../components/QueryStatus';
import { Dialog } from '../../components/Dialog';
import { useListState } from '../../lib/useListState';
import { formatCost, formatDuration, formatTokens, formatTps } from '../../lib/format';
import type { UsageFilters } from '../../types';

export function SessionsPage({ filters }: { filters: UsageFilters }) {
  const { t } = useTranslation();
  const tpsDescription = `${t('TPS = 输出 Token（含推理）÷ 活跃秒数；活跃时长包含工具调用等等待。')} ${t('已扣除可识别的用户答复等待。')}`;
  const list = useListState(filters, 'recent');
  const [selectedId, setSelectedId] = useState<string>();
  const [eventPage, setEventPage] = useState(0);
  const [eventDataOpen, setEventDataOpen] = useState(false);
  const request = { filters, search: list.querySearch, sort: list.sort, descending: list.descending, page: list.page, pageSize: 30 };
  const sessions = useQuery({
    queryKey: ['sessions', request], queryFn: () => getSessions(request), placeholderData: keepPreviousData,
  });
  const detail = useQuery({
    queryKey: ['session-detail', selectedId], queryFn: () => getSessionDetail(selectedId ?? ''), enabled: Boolean(selectedId),
  });
  const events = useQuery({
    queryKey: ['usage-events', selectedId, eventPage],
    queryFn: () => getUsageEvents(selectedId ?? '', eventPage),
    enabled: Boolean(selectedId),
    placeholderData: (previous, previousQuery) => previousQuery?.queryKey[1] === selectedId ? previous : undefined,
  });
  const detailEventMax = Math.max(
    ...(events.data?.items.map((event) => event.totalTokens) ?? [1]),
    1,
  );
  return (
    <section className="feature-page">
      <PageControls
        {...list} onPage={list.setPage} loading={sessions.isFetching} pageSize={30}
        total={sessions.data?.total ?? 0}
        sortOptions={[
          { value: 'recent', label: t('最近活动') }, { value: 'started', label: t('开始时间') },
          { value: 'tokens', label: 'Token' }, { value: 'cost', label: t('成本') },
          { value: 'active_time', label: t('活跃时间') },
          { value: 'tps', label: t('TPS') },
        ]}
      />
      <QueryStatus
        loading={sessions.isLoading && !sessions.data}
        error={sessions.error}
        refreshing={sessions.isFetching && Boolean(sessions.data)}
        onRetry={() => void sessions.refetch()}
      />
      {sessions.data && (
      <div className="data-table-wrap" role="region" aria-label={t('会话')} tabIndex={0} aria-busy={sessions.isFetching}>
        <table className="data-table sessions-table">
          <thead><tr><th>{t('会话')}</th><th>{t('时间')}</th><th>{t('模型')}</th><th>Token</th><th>{t('成本')}</th><th>{t('活跃')}</th><th title={tpsDescription}>{t('TPS')}</th><th>{t('完整性')}</th><th><span className="sr-only">{t('操作')}</span></th></tr></thead>
          <tbody>{sessions.data?.items.map((session) => (
            <tr key={session.id}>
              <td><strong>{session.title}</strong><small>{session.workspaceLabel}{session.archived && <><Archive />{t('归档')}</>}</small></td>
              <td><strong>{session.startedAtMs ? new Date(session.startedAtMs).toLocaleString() : '—'}</strong><small>{t('结束')} · {session.endedAtMs ? new Date(session.endedAtMs).toLocaleString() : '—'}</small></td>
              <td><span className="model-pill">{session.latestModel}</span><small>{session.modelProvider}</small></td>
              <td title={session.totalTokens.toLocaleString()}><strong>{formatTokens(session.totalTokens)}</strong><small>{t('输入')} {formatTokens(session.freshInputTokens)} · {t('缓存')} {formatTokens(session.cachedInputTokens)} · {t('输出')} {formatTokens(session.outputTokens)} · {t('推理')} {formatTokens(session.reasoningTokens)}</small></td>
              <td>{formatCost(session.estimatedCostMicrousd)}{session.unpricedEventCount > 0 && <small className="warning-copy">{t('部分未定价')}</small>}</td>
              <td><strong>{formatDuration(session.activeMs)}</strong><small title={session.activeMethod}>{session.activeIsEstimate ? t('估算') : t('生命周期')}</small></td>
              <td className="tps-value" title={tpsDescription}><strong>{formatTps(session.tokensPerSecond)}</strong>{session.tokensPerSecond != null && session.activeIsEstimate && <small>{t('估算')}</small>}</td>
              <td><span className={`integrity ${session.integrityStatus}`}>{t(session.integrityStatus)}</span></td>
              <td><button type="button" className="icon-button" aria-label={t('查看会话详情')} onClick={() => { setEventPage(0); setEventDataOpen(false); setSelectedId(session.id); }}><ChevronRight /></button></td>
            </tr>
          ))}</tbody>
        </table>
        {!sessions.isLoading && sessions.data?.items.length === 0 && <div className="table-empty">{t('没有匹配的会话。')}</div>}
      </div>
      )}
      {selectedId && (
        <Dialog className="detail-drawer" label={t('会话结构化详情')} onClose={() => setSelectedId(undefined)}>
          <button type="button" className="drawer-close" onClick={() => setSelectedId(undefined)}>{t('关闭')}</button>
          {detail.isLoading && <div className="inline-loading">{t('正在读取结构化统计…')}</div>}
          <QueryStatus loading={false} error={detail.error} onRetry={() => void detail.refetch()} />
          {detail.data && <>
            <h2>{detail.data.session.title}</h2>
            <p>{t('仅显示结构化统计；不读取或显示对话正文。')}</p>
            <div className="detail-metrics">
              <div><span>Token</span><strong>{formatTokens(detail.data.session.totalTokens)}</strong></div>
              <div title={tpsDescription}><span>{t('TPS')}{detail.data.session.tokensPerSecond != null && detail.data.session.activeIsEstimate ? ` · ${t('估算')}` : ''}</span><strong>{formatTps(detail.data.session.tokensPerSecond)}</strong></div>
              <div><span>{t('使用模型')}</span><strong>{detail.data.modelSegments.length}</strong></div>
              <div><span>{t('工具类别')}</span><strong>{detail.data.tools.length}</strong></div>
              <div><span>{t('保留事件')}</span><strong>{detail.data.retainedEventCount}</strong></div>
            </div>
            <h3>{t('解析与完整性')}</h3>
            <div className="compact-list">
              <div><span>{t('解析来源')}</span><strong>{detail.data.parsing.sourceKind} · {detail.data.parsing.sourceStatus}</strong></div>
              <div><span>{t('解析器版本')}</span><strong>v{detail.data.parsing.parserVersion}</strong></div>
              <div><span>{t('解析警告')}</span><strong>{detail.data.parsing.warningCount}{detail.data.parsing.lastErrorCode ? ` · ${detail.data.parsing.lastErrorCode}` : ''}</strong></div>
            </div>
            <h3>{t('Token 时间变化')}</h3>
            <QueryStatus loading={events.isLoading} refreshing={events.isFetching && Boolean(events.data)} error={events.error} onRetry={() => void events.refetch()} />
            {events.data?.items.length === 0 && <p className="section-empty">{t('最近 90 天内没有保留的 Token 事件。历史汇总仍然保留。')}</p>}
            <div className="spark-bars session-spark" role="img" aria-label={t('Token 时间变化')}>
              {[...(events.data?.items ?? [])].reverse().map((event) => {
                return <i key={event.id} style={{ height: `${Math.max(2, (event.totalTokens / detailEventMax) * 100)}%` }} title={`${new Date(event.occurredAtMs).toLocaleString()} · ${event.totalTokens.toLocaleString()}`} />;
              })}
            </div>
            {Boolean(events.data?.items.length) && <details className="chart-data" onToggle={(event) => setEventDataOpen(event.currentTarget.open)}>
              <summary>{t('查看图表数据')}</summary>
              {eventDataOpen && <div className="data-table-wrap" role="region" aria-label={t('Token 时间变化')} tabIndex={0}>
                <table className="data-table">
                  <thead><tr><th>{t('时间')}</th><th>Token</th></tr></thead>
                  <tbody>{events.data?.items.map((event) => <tr key={event.id}><td>{new Date(event.occurredAtMs).toLocaleString()}</td><td>{event.totalTokens.toLocaleString()}</td></tr>)}</tbody>
                </table>
              </div>}
            </details>}
            <div className="event-pager"><button type="button" className="quiet-button" disabled={eventPage === 0 || events.isFetching} onClick={() => setEventPage((value) => value - 1)}>{t('上一页')}</button><span>{eventPage + 1} / {Math.max(1, Math.ceil((events.data?.total ?? 0) / (events.data?.pageSize ?? 100)))}</span><button type="button" className="quiet-button" disabled={events.isFetching || (eventPage + 1) * (events.data?.pageSize ?? 100) >= (events.data?.total ?? 0)} onClick={() => setEventPage((value) => value + 1)}>{t('下一页')}</button></div>
            <h3><Clock3 />{t('活跃时间段')}</h3>
            {detail.data.activitySegments.length === 0 && <p className="section-empty">{t('暂无结构化记录。')}</p>}
            <div className="compact-list">{detail.data.activitySegments.map((segment) => (
              <div key={segment.segmentIndex}><span>{new Date(segment.startedAtMs).toLocaleString()}</span><strong>{formatDuration(segment.activeMs)} · {segment.isEstimate ? t('估算') : t('生命周期')}</strong></div>
            ))}</div>
            <h3>{t('模型记录')}</h3>
            {detail.data.modelSegments.length === 0 && <p className="section-empty">{t('暂无结构化记录。')}</p>}
            <div className="compact-list">{detail.data.modelSegments.map((segment) => (
              <div key={segment.segmentIndex}><span>{segment.provider} / {segment.model}</span><strong>{formatTokens(segment.inputTokens + segment.outputTokens)}</strong></div>
            ))}</div>
            <h3>{t('工具统计')}</h3>
            {detail.data.tools.length === 0 && <p className="section-empty">{t('暂无结构化记录。')}</p>}
            <div className="compact-list">{detail.data.tools.map((tool) => (
              <div key={`${tool.toolName}-${tool.category}`}><span>{tool.toolName} · {tool.category}</span><strong>{tool.callCount}</strong></div>
            ))}</div>
          </>}
        </Dialog>
      )}
    </section>
  );
}
