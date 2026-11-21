import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { App } from './App';
import i18n from './i18n';
import type { DashboardSnapshot, HeatmapSnapshot } from './types';

const api = vi.hoisted(() => ({
  getBootstrapStatus: vi.fn(),
  getDashboard: vi.fn(),
  getSyncStatus: vi.fn(),
  startSync: vi.fn(),
  cancelSync: vi.fn(),
  getAppPreferences: vi.fn(),
  getWorkspaceCatalog: vi.fn(),
  saveAppPreferences: vi.fn(),
  getHeatmap: vi.fn(),
  getSessions: vi.fn(),
  getWorkspaces: vi.fn(),
  getModels: vi.fn(),
  getTools: vi.fn(),
  getSessionDetail: vi.fn(),
  getUsageEvents: vi.fn(),
  exportData: vi.fn(),
  writeChartPng: vi.fn(),
}));

const nativeRuntime = vi.hoisted(() => ({
  enabled: false,
  listeners: new Map<string, () => void>(),
}));

vi.mock('./api', async (importOriginal) => ({
  ...await importOriginal<typeof import('./api')>(),
  ...api,
  isTauriRuntime: () => nativeRuntime.enabled,
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async (event: string, handler: () => void) => {
    nativeRuntime.listeners.set(event, handler);
    return () => nativeRuntime.listeners.delete(event);
  }),
}));
vi.mock('./components/UsageTrendChart', () => ({
  UsageTrendChart: () => <div data-testid="trend-chart" />,
}));

const baseSnapshot: DashboardSnapshot = {
  resolvedRange: {
    startMs: 1_787_000_000_000,
    endMs: 1_788_000_000_000,
    startLocalDate: '2026-06-10',
    endLocalDate: '2026-07-10',
    calendarDays: 30,
    granularity: 'day',
  },
  hero: {
    realTotalTokens: 0, inputTokens: 0, freshInputTokens: 0, cachedInputTokens: 0,
    outputTokens: 0, reasoningTokens: 0, unpricedEventCount: 0, sessionCount: 0,
    activeMs: 0, activeDays: 0, averageTokensPerDay: 0, averageSessionsPerDay: 0,
    averageActiveMsPerDay: 0, peakDayTokens: 0, longestActiveStreakDays: 0,
  },
  trend: [],
  filterOptions: { workspaces: [], providers: [], models: [], vendors: ['openai', 'deepseek'] },
  dataState: 'complete',
  generatedAtMs: Date.now(),
};

function renderApp() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(<QueryClientProvider client={client}><App /></QueryClientProvider>);
}

describe('App data states', () => {
  beforeEach(async () => {
    vi.clearAllMocks();
    nativeRuntime.enabled = false;
    nativeRuntime.listeners.clear();
    await i18n.changeLanguage('zh-CN');
    api.getBootstrapStatus.mockResolvedValue({
      appVersion: '2.1.0', platform: 'windows', dataDirectory: 'C:/Users/test/AppData/Local/Chronolume/v2',
      databasePath: '', schemaVersion: 1, databaseSizeBytes: 0,
    });
    api.getSyncStatus.mockResolvedValue({
      phase: 'idle', filesTotal: 0, filesCompleted: 0, bytesTotal: 0, bytesRead: 0,
      recordsWritten: 0, recordsSkipped: 0, parseFailures: 0, fileErrors: 0,
      speedBytesPerSecond: 0, updatedAtMs: Date.now(), cancelRequested: false,
    });
    api.getAppPreferences.mockResolvedValue({ idleGapMinutes: 30, visibleWorkspaceIds: [] });
    api.getWorkspaceCatalog.mockResolvedValue([]);
    api.saveAppPreferences.mockImplementation(async (preferences) => preferences);
    api.getHeatmap.mockResolvedValue({
      metric: 'tokens', span: 'year', startDate: '2026-07-01', endDate: '2026-07-10',
      maxValue: 0, points: [],
    });
    api.startSync.mockResolvedValue({ phase: 'completed' });
  });

  it('renders the explicit empty state without synthetic usage', async () => {
    api.getDashboard.mockResolvedValue({ ...baseSnapshot, dataState: 'empty' });
    renderApp();
    expect(await screen.findByText('这个范围内还没有用量')).toBeInTheDocument();
    expect(screen.getByLabelText('用量汇总').querySelector('[title="0"]')).toHaveTextContent('0');
    expect(screen.getByText('活跃热力图')).toBeInTheDocument();
    expect(api.getHeatmap).toHaveBeenCalledWith(expect.anything(), 'tokens', 'year');
    const analytics = screen.getByTestId('overview-analytics-grid');
    const heatmap = within(analytics).getByText('活跃热力图').closest('section');
    const trend = await within(analytics).findByTestId('trend-chart');
    expect(analytics.firstElementChild).toBe(heatmap);
    expect(analytics.lastElementChild).toContainElement(trend);
  });

  it('does not show a partial-data notice but still labels stale snapshots', async () => {
    api.getDashboard.mockResolvedValue({ ...baseSnapshot, dataState: 'partial' });
    const { unmount } = renderApp();
    expect(await screen.findByText('真实总 Token')).toBeInTheDocument();
    expect(screen.queryByText('部分会话缺少完整 Token 或生命周期记录；当前仅展示可验证的真实数据。')).not.toBeInTheDocument();
    unmount();

    api.getDashboard.mockResolvedValue({ ...baseSnapshot, generatedAtMs: Date.now() - 10 * 60_000 });
    renderApp();
    expect(await screen.findByText('当前快照已超过 5 分钟，可刷新获取最新统计。')).toBeInTheDocument();
  });

  it('keeps the previous heatmap metric out of a pending metric query', async () => {
    const snapshot: HeatmapSnapshot = {
      metric: 'tokens', span: 'year', startDate: '2026-07-10', endDate: '2026-07-10',
      maxValue: 100, points: [{
        date: '2026-07-10', value: 100, totalTokens: 100, sessionCount: 1, activeMs: 100,
      }],
    };
    let finish!: (snapshot: HeatmapSnapshot) => void;
    api.getDashboard.mockResolvedValue(baseSnapshot);
    api.getHeatmap
      .mockResolvedValueOnce(snapshot)
      .mockImplementationOnce(() => new Promise<HeatmapSnapshot>((resolve) => { finish = resolve; }));
    renderApp();
    expect(await screen.findByLabelText('2026-07-10，100 Token')).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText('热力图指标'), { target: { value: 'sessions' } });
    expect(screen.queryByLabelText('2026-07-10，100 Token')).not.toBeInTheDocument();
    expect(screen.getByTestId('overview-analytics-grid')).toHaveAttribute('aria-busy', 'true');
    await act(async () => finish({ ...snapshot, metric: 'sessions', points: [{ ...snapshot.points[0], value: 1 }] }));
    expect(await screen.findByLabelText('2026-07-10，1 个会话')).toBeInTheDocument();
  });

  it('selects and clears session vendors after paging, including empty results', async () => {
    api.getDashboard.mockResolvedValue(baseSnapshot);
    api.getSessions.mockImplementation(async (query) => ({
      items: [], page: query.page, pageSize: 30,
      total: query.filters.modelVendors?.includes('deepseek') ? 0 : 60,
    }));
    renderApp();
    fireEvent.click(screen.getByRole('button', { name: '会话' }));
    const trigger = await screen.findByRole('button', { name: '模型供应商 全部供应商' });
    await waitFor(() => expect(screen.getByRole('button', { name: '下一页' })).toBeEnabled());
    fireEvent.click(screen.getByRole('button', { name: '下一页' }));
    await waitFor(() => expect(api.getSessions).toHaveBeenLastCalledWith(expect.objectContaining({ page: 1 })));
    fireEvent.click(trigger);
    fireEvent.click(await screen.findByRole('checkbox', { name: 'DeepSeek' }));
    await waitFor(() => expect(api.getSessions).toHaveBeenLastCalledWith(expect.objectContaining({
      filters: expect.objectContaining({ modelVendors: ['deepseek'] }), page: 0,
    })));
    fireEvent.click(screen.getByRole('checkbox', { name: 'OpenAI' }));
    await waitFor(() => expect(api.getSessions).toHaveBeenLastCalledWith(expect.objectContaining({
      filters: expect.objectContaining({ modelVendors: ['deepseek', 'openai'] }), page: 0,
    })));
    fireEvent.click(screen.getByRole('button', { name: '清除供应商筛选' }));
    await waitFor(() => expect(api.getSessions).toHaveBeenLastCalledWith(expect.objectContaining({ filters: {
      range: { preset: 'last30_days', liveEnd: false }, archived: 'all',
    }, page: 0 })));
    expect(screen.queryByText('界面加载失败')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: '模型供应商 全部供应商' })).toHaveFocus();
  });

  it('keeps vendor selection available and shared across every usage page', async () => {
    api.getDashboard.mockResolvedValue(baseSnapshot);
    const emptyPage = { items: [], page: 0, pageSize: 30, total: 0 };
    api.getSessions.mockResolvedValue(emptyPage);
    api.getWorkspaces.mockResolvedValue(emptyPage);
    api.getModels.mockResolvedValue(emptyPage);
    api.getTools.mockResolvedValue({ totalCalls: 0, uniqueTools: 0, categories: [], trend: [], topTools: [] });
    renderApp();
    fireEvent.click(await screen.findByRole('button', { name: '模型供应商 全部供应商' }));
    fireEvent.click(await screen.findByRole('checkbox', { name: 'OpenAI' }));
    const expectedFilters = {
      range: { preset: 'last30_days', liveEnd: false }, archived: 'all', modelVendors: ['openai'],
    };
    await waitFor(() => expect(api.getDashboard).toHaveBeenLastCalledWith(expectedFilters));
    expect(api.getHeatmap).toHaveBeenLastCalledWith(expectedFilters, 'tokens', 'year');
    for (const [name, query] of [
      ['项目', api.getWorkspaces], ['会话', api.getSessions], ['模型与成本', api.getModels],
    ] as const) {
      fireEvent.click(screen.getByRole('button', { name }));
      await waitFor(() => expect(query).toHaveBeenCalledWith(expect.objectContaining({ filters: expectedFilters })));
      expect(screen.getByRole('button', { name: '模型供应商 OpenAI' })).toBeInTheDocument();
      expect(screen.queryByRole('combobox', { name: '模型' })).not.toBeInTheDocument();
    }
    fireEvent.click(screen.getByRole('button', { name: '工具与活动' }));
    await waitFor(() => expect(api.getTools).toHaveBeenCalledWith(expectedFilters));
    expect(screen.getByRole('button', { name: '模型供应商 OpenAI' })).toBeInTheDocument();
  });

  it('shows a retryable error instead of silently substituting data', async () => {
    api.getDashboard.mockRejectedValue(new Error('synthetic query failure'));
    renderApp();
    expect(await screen.findByRole('alert')).toHaveTextContent('synthetic query failure');
    expect(screen.getByRole('button', { name: '重试查询' })).toBeInTheDocument();
  });

  it('opens settings when the native macOS Settings menu event is emitted', async () => {
    nativeRuntime.enabled = true;
    api.getDashboard.mockResolvedValue(baseSnapshot);
    const { unmount } = renderApp();

    await waitFor(() => expect(nativeRuntime.listeners.has('chronolume-open-settings')).toBe(true));
    act(() => nativeRuntime.listeners.get('chronolume-open-settings')?.());

    expect(await screen.findByRole('heading', { name: '外观与偏好' })).toBeInTheDocument();
    unmount();
  });
});
