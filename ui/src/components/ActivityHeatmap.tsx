import type { HeatmapMetric, HeatmapSnapshot, HeatmapSpan } from '../types';
import { useLayoutEffect, useMemo, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { formatDuration, formatExact, formatTokens } from '../lib/format';
import { QueryStatus } from './QueryStatus';
import { DragScroll } from './DragScroll';
import i18n from '../i18n';

interface ActivityHeatmapProps {
  snapshot?: HeatmapSnapshot;
  metric: HeatmapMetric;
  span: HeatmapSpan;
  loading: boolean;
  error?: unknown;
  onRetry?: () => void;
  onMetric: (metric: HeatmapMetric) => void;
  onSpan: (span: HeatmapSpan) => void;
}

export function ActivityHeatmap({ snapshot, metric, span, loading, error, onRetry = () => undefined, onMetric, onSpan }: ActivityHeatmapProps) {
  const { t } = useTranslation();
  const scrollRef = useRef<HTMLDivElement>(null);
  const points = snapshot?.points;
  const positiveValues = useMemo(() => (points ?? [])
    .map((point) => point.value)
    .filter((value) => value > 0)
    .sort((left, right) => left - right), [points]);

  useLayoutEffect(() => {
    const scroll = scrollRef.current;
    if (!scroll || loading) return;
    scroll.scrollLeft = Math.max(0, scroll.scrollWidth - scroll.clientWidth);
  }, [loading, metric, snapshot?.endDate, snapshot?.points.length, span]);

  return (
    <section className={`feature-card heatmap-card metric-${metric.replace('_', '-')}`}>
      <div className="feature-card-heading">
        <div><span>{t('活跃度')}</span><h2>{t('活跃热力图')}</h2></div>
        <div className="segmented-controls">
          <select value={metric} aria-label={t('热力图指标')} onChange={(event) => onMetric(event.target.value as HeatmapMetric)}>
            <option value="sessions">{t('会话数')}</option>
            <option value="tokens">Token</option>
            <option value="active_time">{t('活跃时间')}</option>
          </select>
          <select value={span} aria-label={t('热力图范围')} onChange={(event) => onSpan(event.target.value as HeatmapSpan)}>
            <option value="week">{t('周')}</option>
            <option value="month">{t('月')}</option>
            <option value="year">{t('年')}</option>
          </select>
        </div>
      </div>
      <QueryStatus loading={loading} error={error} onRetry={onRetry} />
      {snapshot && snapshot.points.length > 0 ? <>
        <div className="heatmap-plot">
          <DragScroll axis="x" label={t('活跃热力图')} className="heatmap-scroll" scrollRef={scrollRef}>
            <div className={`heatmap-grid span-${span}`}>
              {snapshot.points.map((point) => {
                const level = heatLevel(metric, point.value, snapshot.maxValue, positiveValues);
                const value = metric === 'active_time' && point.activeTimingAvailable === false ? '—' : metricValue(metric, point.value);
                return (
                  <div
                    key={point.date}
                    className={`heat-cell level-${level}`}
                    title={`${point.date} · ${value}`}
                    aria-label={`${point.date}，${value}`}
                    role="img"
                  />
                );
              })}
            </div>
          </DragScroll>
        </div>
        <div className="heatmap-footer">
          <span>{snapshot.startDate} – {snapshot.endDate}</span>
          <div className="heatmap-legend"><span>{t('少')}</span>{[0, 1, 2, 3, 4].map((level) => <i key={level} className={`level-${level}`} />)}<span>{t('多')}</span></div>
        </div>
      </> : !loading && !error && <div className="chart-empty">{t('当前筛选范围内还没有可绘制的数据。')}</div>}
    </section>
  );
}

function heatLevel(metric: HeatmapMetric, value: number, maxValue: number, positiveValues: number[]): number {
  if (value <= 0 || maxValue <= 0) return 0;
  if (metric === 'sessions') return Math.min(4, Math.max(1, Math.ceil((value / maxValue) * 4)));

  // Token 和活跃时间通常是长尾分布。按正值的百分位着色，避免一个峰值让全年
  // 其余方块都落在同一档；会话数沿用原来的线性刻度。
  let lower = 0;
  let upper = positiveValues.length;
  while (lower < upper) {
    const middle = Math.floor((lower + upper) / 2);
    if (positiveValues[middle] <= value) lower = middle + 1;
    else upper = middle;
  }
  return Math.min(4, Math.max(1, Math.ceil((lower / positiveValues.length) * 4)));
}

function metricValue(metric: HeatmapMetric, value: number): string {
  if (metric === 'tokens') return `${formatTokens(value)} Token`;
  if (metric === 'active_time') return formatDuration(value);
  return i18n.t('{{count}} 个会话', { count: value, formatted: formatExact(value) });
}
