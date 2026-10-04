import { lazy, Suspense, useCallback, useEffect, useMemo, useState } from 'react';
import { keepPreviousData, useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { listen } from '@tauri-apps/api/event';
import { useTranslation } from 'react-i18next';
import {
  BarChart3,
  Cpu,
  Database,
  Gauge,
  FolderKanban,
  Settings2,
  Wrench,
  X,
} from 'lucide-react';
import {
  cancelSync,
  getBootstrapStatus,
  getDashboard,
  getHeatmap,
  getAppPreferences,
  getSyncStatus,
  getWorkspaceCatalog,
  isTauriRuntime,
  saveAppPreferences,
  startSync,
} from './api';
import { DashboardFilters } from './components/DashboardFilters';
import { ActivityHeatmap } from './components/ActivityHeatmap';
import { ExportControls } from './components/ExportControls';
import { SyncStrip } from './components/SyncStrip';
import { UsageHero } from './components/UsageHero';
import { WorkspaceVisibilityDialog } from './components/WorkspaceVisibilityDialog';
import { ActionFeedback } from './components/ActionFeedback';
import { ErrorBoundary } from './components/ErrorBoundary';
import { QueryStatus } from './components/QueryStatus';
import { errorMessage } from './lib/errors';
import { invalidateUsageData as invalidateUsageQueries } from './lib/queries';
import { formatBytes } from './lib/format';
import type { HeatmapMetric, HeatmapSpan, UsageFilters } from './types';

const ACTIVE_SYNC_PHASES = new Set(['detecting', 'planning', 'importing', 'rolling_up']);

type PageId = 'overview' | 'projects' | 'sessions' | 'models' | 'activity' | 'data' | 'settings';

const UsageTrendChart = lazy(() => import('./components/UsageTrendChart').then((module) => ({ default: module.UsageTrendChart })));
const ProjectsPage = lazy(() => import('./features/projects/ProjectsPage').then((module) => ({ default: module.ProjectsPage })));
const SessionsPage = lazy(() => import('./features/sessions/SessionsPage').then((module) => ({ default: module.SessionsPage })));
const ModelsPage = lazy(() => import('./features/models/ModelsPage').then((module) => ({ default: module.ModelsPage })));
const ActivityPage = lazy(() => import('./features/activity/ActivityPage').then((module) => ({ default: module.ActivityPage })));
const DataPage = lazy(() => import('./features/data/DataPage').then((module) => ({ default: module.DataPage })));
const SettingsPage = lazy(() => import('./features/settings/SettingsPage').then((module) => ({ default: module.SettingsPage })));

const DEFAULT_FILTERS: UsageFilters = {
  range: { preset: 'last30_days', liveEnd: false },
  archived: 'all',
};

export function App() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [activePage, setActivePage] = useState<PageId>('overview');
  const [filters, setFilters] = useState<UsageFilters>(DEFAULT_FILTERS);
  const [workspaceDialogOpen, setWorkspaceDialogOpen] = useState(false);
  const [dismissedNotice, setDismissedNotice] = useState<string>();
  const [eventError, setEventError] = useState<unknown>();
  const [heatmapMetric, setHeatmapMetric] = useState<HeatmapMetric>('tokens');
  const [heatmapSpan, setHeatmapSpan] = useState<HeatmapSpan>('year');
  const invalidateUsageData = useCallback(() => {
    invalidateUsageQueries(queryClient);
  }, [queryClient]);
  const bootstrap = useQuery({ queryKey: ['bootstrap'], queryFn: getBootstrapStatus });
  const preferences = useQuery({ queryKey: ['app-preferences'], queryFn: getAppPreferences });
  const workspaceCatalog = useQuery({ queryKey: ['workspace-catalog'], queryFn: getWorkspaceCatalog });
  const dashboard = useQuery({
    queryKey: ['dashboard', filters],
    queryFn: () => getDashboard(filters),
    placeholderData: keepPreviousData,
  });
  const heatmap = useQuery({
    queryKey: ['heatmap', filters, heatmapMetric, heatmapSpan],
    queryFn: () => getHeatmap(filters, heatmapMetric, heatmapSpan),
    enabled: activePage === 'overview',
    placeholderData: keepPreviousData,
  });
  const sync = useQuery({
    queryKey: ['sync-status'],
    queryFn: getSyncStatus,
    refetchInterval: (query) =>
      ACTIVE_SYNC_PHASES.has(query.state.data?.phase ?? '') ? 500 : 5_000,
  });

  const refreshMutation = useMutation({
    mutationFn: () => startSync('incremental'),
    onSuccess: (status) => {
      queryClient.setQueryData(['sync-status'], status);
      invalidateUsageData();
    },
  });
  const cancelMutation = useMutation({
    mutationFn: cancelSync,
    onSuccess: (status) => queryClient.setQueryData(['sync-status'], status),
  });
  const saveWorkspaceVisibility = useMutation({
    mutationFn: (visibleWorkspaceIds: string[]) => saveAppPreferences({
      idleGapMinutes: preferences.data?.idleGapMinutes ?? 30,
      visibleWorkspaceIds,
    }),
    onSuccess: (saved) => {
      queryClient.setQueryData(['app-preferences'], saved);
      setWorkspaceDialogOpen(false);
      setFilters((current) => saved.visibleWorkspaceIds.includes(current.workspaceId ?? '')
        ? current
        : cascadeWorkspaceReset(current));
      void queryClient.invalidateQueries({ queryKey: ['workspaces'] });
    },
  });

  useEffect(() => {
    if (filters.range.preset !== 'custom' || !filters.range.liveEnd) return;
    const timer = window.setInterval(invalidateUsageData, 30_000);
    return () => window.clearInterval(timer);
  }, [filters.range.preset, filters.range.liveEnd, invalidateUsageData]);

  useEffect(() => {
    if (!isTauriRuntime()) return undefined;
    const unlisten = Promise.allSettled([
      listen('usage-sync-completed', () => {
        invalidateUsageData();
        void queryClient.invalidateQueries({ queryKey: ['sync-status'] });
      }),
      listen('usage-sync-failed', () => {
        void queryClient.invalidateQueries({ queryKey: ['sync-status'] });
      }),
      listen('chronolume-open-settings', () => {
        setActivePage('settings');
      }),
    ]).then((results) => {
      const failure = results.find((result) => result.status === 'rejected');
      if (failure?.status === 'rejected') setEventError(failure.reason);
      return results.flatMap((result) => result.status === 'fulfilled' ? [result.value] : []);
    });
    return () => {
      void unlisten.then((listeners) => listeners.forEach((stop) => stop()));
    };
  }, [invalidateUsageData, queryClient]);

  useEffect(() => {
    if (!isTauriRuntime()) return undefined;
    const timer = window.setInterval(() => {
      if (ACTIVE_SYNC_PHASES.has(sync.data?.phase ?? '')) return;
      void startSync('incremental').then(
        (status) => {
          queryClient.setQueryData(['sync-status'], status);
          invalidateUsageData();
        },
        () => void queryClient.invalidateQueries({ queryKey: ['sync-status'] }),
      );
    }, 30_000);
    return () => window.clearInterval(timer);
  }, [invalidateUsageData, queryClient, sync.data?.phase]);

  const snapshot = dashboard.data;
  const syncing = ACTIVE_SYNC_PHASES.has(sync.data?.phase ?? '');
  const stale = snapshot != null && Date.now() - snapshot.generatedAtMs > 5 * 60_000;
  const noticeKind = stale ? 'stale' : undefined;
  const visibleWorkspaceIds = preferences.data?.visibleWorkspaceIds ?? [];
  const visibleWorkspaceOptions = useMemo(() => {
    const visible = new Set(visibleWorkspaceIds);
    return (workspaceCatalog.data ?? [])
      .filter((option) => visible.has(option.value))
      .map(({ value, label }) => ({ value, label }));
  }, [visibleWorkspaceIds, workspaceCatalog.data]);
  const dashboardFilterOptions = useMemo(() => ({
    workspaces: visibleWorkspaceOptions,
    providers: snapshot?.filterOptions.providers ?? [],
    models: snapshot?.filterOptions.models ?? [],
  }), [snapshot?.filterOptions.models, snapshot?.filterOptions.providers, visibleWorkspaceOptions]);
  const statusLabel = useMemo(() => {
    if (!isTauriRuntime()) return t('浏览器预览');
    if (syncing) return t('后台索引中');
    if (dashboard.isError || sync.data?.phase === 'failed') return t('需要关注');
    return t('本地数据');
  }, [dashboard.isError, sync.data?.phase, syncing, t]);

  const refresh = () => {
    refreshMutation.mutate();
    void dashboard.refetch();
  };
  const pageCopy: Record<PageId, { eyebrow: string; title: string; description: string }> = {
    overview: { eyebrow: 'CHRONOLUME · LOCAL FIRST', title: t('本地用量总览'), description: t('直接读取本机 Codex 数据；原始消息与工具参数不会进入分析库。') },
    projects: { eyebrow: 'WORKSPACES', title: t('项目与工作区'), description: t('按真实工作区汇总 Token、成本、活跃时间和会话。') },
    sessions: { eyebrow: 'SESSIONS', title: t('会话与事件'), description: t('只显示结构化统计；最近 90 天 Token 事件由服务端分页保留。') },
    models: { eyebrow: 'MODELS & PRICING', title: t('模型与成本'), description: t('检查模型分布、未定价事件和本地价格覆盖。') },
    activity: { eyebrow: 'ACTIVITY', title: t('工具与改动分析'), description: t('命令参数只在内存分类，数据库仅保存类别和计数。') },
    data: { eyebrow: 'DATA HEALTH', title: t('数据与诊断'), description: t('查看来源能力、索引进度、增量水位和完整性。') },
    settings: { eyebrow: 'PREFERENCES', title: t('外观与偏好'), description: t('界面偏好只保存在本机。') },
  };
  const copy = pageCopy[activePage];
  const filterVisible = !['data', 'settings'].includes(activePage);
  const exportScope = ({
    overview: 'dashboard', projects: 'workspaces', sessions: 'sessions', models: 'models',
    activity: 'tools', data: undefined, settings: undefined,
  } as const)[activePage];

  return (
    <div className="app-shell">
      <a className="skip-link" href="#main-content">{t('跳转到主要内容')}</a>
      <aside className="sidebar" aria-label={t('主导航')}>
        <div className="brand-mark" aria-hidden="true">CL</div>
        <nav>
          <NavButton icon={<BarChart3 />} label={t('总览')} active={activePage === 'overview'} onSelect={() => setActivePage('overview')} />
          <NavButton icon={<FolderKanban />} label={t('项目')} active={activePage === 'projects'} onSelect={() => setActivePage('projects')} />
          <NavButton icon={<Gauge />} label={t('会话')} active={activePage === 'sessions'} onSelect={() => setActivePage('sessions')} />
          <NavButton icon={<Cpu />} label={t('模型与成本')} active={activePage === 'models'} onSelect={() => setActivePage('models')} />
          <NavButton icon={<Wrench />} label={t('工具与活动')} active={activePage === 'activity'} onSelect={() => setActivePage('activity')} />
          <NavButton icon={<Database />} label={t('数据')} active={activePage === 'data'} onSelect={() => setActivePage('data')} />
          <NavButton icon={<Settings2 />} label={t('设置')} active={activePage === 'settings'} onSelect={() => setActivePage('settings')} />
        </nav>
        <div className="sidebar-spacer" />
      </aside>

      <main id="main-content" className="content" tabIndex={-1}>
        <header className="page-heading">
          <div>
            <p className="eyebrow">{copy.eyebrow}</p>
            <h1>{copy.title}</h1>
            <p>{copy.description}</p>
          </div>
          <div className="header-actions">
            <ExportControls scope={exportScope} filters={filters} allowPng={activePage === 'overview' && Boolean(snapshot?.trend.length)} disabled={dashboard.isFetching || !snapshot || dashboard.isError} />
            <span className={`status-pill${syncing ? ' syncing' : ''}${dashboard.isError || sync.data?.phase === 'failed' ? ' attention' : ''}`}><i aria-hidden="true" />{statusLabel}</span>
          </div>
        </header>

        {isTauriRuntime() ? <SyncStrip status={sync.data} cancelling={cancelMutation.isPending} onCancel={() => cancelMutation.mutate()} /> :
          <div className="preview-notice" role="note"><Database /><span>{t('浏览器预览不读取本机数据。请在桌面应用中查看用量、同步和导出。')}</span></div>}
        <ActionFeedback error={refreshMutation.error ?? cancelMutation.error ?? eventError} />
        {sync.isError && <QueryStatus loading={false} error={sync.error} onRetry={() => void sync.refetch()} />}

        {filterVisible && <DashboardFilters
            filters={filters}
            options={dashboardFilterOptions}
            refreshing={dashboard.isFetching || syncing || refreshMutation.isPending}
            onChange={setFilters}
            onRefresh={refresh}
            onManageWorkspaces={() => { saveWorkspaceVisibility.reset(); setWorkspaceDialogOpen(true); }}
          />}

        {activePage === 'overview' && dashboard.isLoading && !snapshot && <DashboardSkeleton />}

        {activePage === 'overview' && dashboard.isError && !snapshot && (
          <section className="state-card error-state" role="alert">
            <strong>{t('无法读取本地统计')}</strong>
            <p>{errorMessage(dashboard.error)}</p>
            <button type="button" onClick={() => void dashboard.refetch()}>{t('重试查询')}</button>
          </section>
        )}

        {activePage === 'overview' && snapshot && (
          <>
            {dashboard.isError && <QueryStatus loading={false} error={dashboard.error} onRetry={() => void dashboard.refetch()} />}
            {noticeKind && dismissedNotice !== noticeKind && (
              <div className="data-notice" role="status">
                <span>{t(dashboard.isFetching ? '展示的是上次生成的快照，后台正在刷新。' : '当前快照已超过 5 分钟，可刷新获取最新统计。')}</span>
                <button type="button" aria-label={t('关闭提示')} onClick={() => setDismissedNotice(noticeKind)}><X /></button>
              </div>
            )}
            <UsageHero hero={snapshot.hero} partial={snapshot.dataState === 'partial'} />
            {snapshot.dataState === 'empty' && (
              <section className="state-card empty-state">
                <Gauge />
                <div className="empty-state-copy">
                  <strong>{t('这个范围内还没有用量')}</strong>
                  <p>{t('可以扩大时间范围、清除筛选，或手动触发一次增量同步。')}</p>
                  <button type="button" className="quiet-button" onClick={() => setFilters({ range: { preset: 'all', liveEnd: false }, archived: 'all' })}>{t('查看全部历史')}</button>
                </div>
              </section>
            )}
            <div className="overview-analytics-grid" data-testid="overview-analytics-grid">
              <ActivityHeatmap
                snapshot={heatmap.data}
                metric={heatmapMetric}
                span={heatmapSpan}
                loading={heatmap.isLoading && !heatmap.data}
                error={heatmap.error}
                onRetry={() => void heatmap.refetch()}
                onMetric={setHeatmapMetric}
                onSpan={setHeatmapSpan}
              />
              <ErrorBoundary><Suspense fallback={<div className="skeleton chart-skeleton" role="status" aria-label={t('正在加载页面数据…')} />}>
                <UsageTrendChart trend={snapshot.trend} granularity={snapshot.resolvedRange.granularity} />
              </Suspense></ErrorBoundary>
            </div>
          </>
        )}

        <ErrorBoundary key={activePage}><Suspense fallback={<DashboardSkeleton />}>
          {activePage === 'projects' && <ProjectsPage
              filters={filters}
              visibleWorkspaceIds={visibleWorkspaceIds}
              onManageWorkspaces={() => { saveWorkspaceVisibility.reset(); setWorkspaceDialogOpen(true); }}
              onOpenWorkspace={(workspaceId) => {
                setFilters((current) => ({
                  ...current,
                  workspaceId,
                  modelProvider: undefined,
                  model: undefined,
                }));
                setActivePage('overview');
              }}
            />}
          {activePage === 'sessions' && <SessionsPage filters={filters} />}
          {activePage === 'models' && <ModelsPage filters={filters} />}
          {activePage === 'activity' && <ActivityPage filters={filters} />}
          {activePage === 'data' && <DataPage />}
          {activePage === 'settings' && <SettingsPage />}
        </Suspense></ErrorBoundary>

        <footer className="app-footer">
          <span>Chronolume {bootstrap.data?.appVersion ?? '2.1.9'}</span>
          <span>Schema v{bootstrap.data?.schemaVersion ?? '…'}</span>
          <span>{bootstrap.data ? formatBytes(bootstrap.data.databaseSizeBytes) : '—'} {t('本地索引')}</span>
          {dashboard.isFetching && <span className="footer-refreshing">{t('正在刷新查询')}</span>}
        </footer>
      </main>
      {workspaceDialogOpen && <WorkspaceVisibilityDialog
          options={workspaceCatalog.data ?? []}
          selectedIds={visibleWorkspaceIds}
          saving={saveWorkspaceVisibility.isPending}
          loading={workspaceCatalog.isLoading || preferences.isLoading}
          error={workspaceCatalog.error ?? preferences.error}
          saveError={saveWorkspaceVisibility.error}
          onRetry={() => { void workspaceCatalog.refetch(); void preferences.refetch(); }}
          onClose={() => setWorkspaceDialogOpen(false)}
          onSave={(ids) => saveWorkspaceVisibility.mutate(ids)}
        />}
    </div>
  );
}

function cascadeWorkspaceReset(filters: UsageFilters): UsageFilters {
  return { ...filters, workspaceId: undefined, modelProvider: undefined, model: undefined };
}

function NavButton({
  icon,
  label,
  active = false,
  onSelect,
}: {
  icon: React.ReactNode;
  label: string;
  active?: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      className={active ? 'nav-button nav-button-active' : 'nav-button'}
      type="button"
      aria-label={label}
      title={label}
      aria-current={active ? 'page' : undefined}
      onClick={onSelect}
    >
      {icon}
      <span className="nav-tooltip" aria-hidden="true">{label}</span>
    </button>
  );
}

function DashboardSkeleton() {
  const { t } = useTranslation();
  return (
    <div className="dashboard-skeleton" aria-label={t('正在加载本地统计')} aria-busy="true">
      <div className="skeleton hero-skeleton" />
      <div className="skeleton chart-skeleton" />
    </div>
  );
}
