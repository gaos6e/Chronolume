import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import i18n from '../i18n';
import type { UsageFilters } from '../types';
import { ActivityPage } from './activity/ActivityPage';
import { DataPage } from './data/DataPage';
import { ModelsPage } from './models/ModelsPage';
import { ProjectsPage } from './projects/ProjectsPage';
import { SessionsPage } from './sessions/SessionsPage';

const api = vi.hoisted(() => ({
  getBootstrapStatus: vi.fn(),
  getSourceSettings: vi.fn(),
  saveSourceSettings: vi.fn(),
  getWorkspaces: vi.fn(),
  updateWorkspaceSettings: vi.fn(),
  getSessions: vi.fn(),
  getSessionDetail: vi.fn(),
  getUsageEvents: vi.fn(),
  getModels: vi.fn(),
  listModelPrices: vi.fn(),
  saveModelPrice: vi.fn(),
  deleteModelPrice: vi.fn(),
  restoreBuiltinPrice: vi.fn(),
  previewPriceUpdate: vi.fn(),
  applyPriceUpdate: vi.fn(),
  getHeatmap: vi.fn(),
  getTools: vi.fn(),
  getDiagnostics: vi.fn(),
  startSync: vi.fn(),
  getSyncStatus: vi.fn(),
  clearAnalysis: vi.fn(),
}));

vi.mock('../api', () => ({
  ...api,
  isTauriRuntime: () => true,
}));

const filters: UsageFilters = {
  range: { preset: 'last30_days', liveEnd: false },
  archived: 'all',
};

function renderPage(node: React.ReactNode) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(<QueryClientProvider client={client}>{node}</QueryClientProvider>);
}

describe('feature pages', () => {
  it('filters official price rows and keeps the source link and zero rates visible', async () => {
    api.getModels.mockResolvedValue({ items: [], total: 0, page: 0, pageSize: 30 });
    api.listModelPrices.mockResolvedValue([
      {provider:'deepseek',pricingId:'deepseek-v4-pro',displayName:'DeepSeek V4 Pro',inputPerMillionUsd:'1.32',outputPerMillionUsd:'3.96',cacheReadPerMillionUsd:'0.044',isBuiltin:true,isDeleted:false,isOverridden:false,revision:1,sourceUrl:'https://api-docs.deepseek.com/quick_start/pricing/',pricingNote:'高峰价'},
      {provider:'opencode',pricingId:'deepseek-v4.1-flash-free',displayName:'DeepSeek Free',inputPerMillionUsd:'0',outputPerMillionUsd:'0',cacheReadPerMillionUsd:'0',isBuiltin:true,isDeleted:false,isOverridden:false,revision:1,sourceUrl:'https://opencode.ai/v2/docs/console/models/'},
    ]);
    renderPage(<ModelsPage filters={filters} />);
    fireEvent.click(screen.getByRole('tab', { name: '价格表' }));
    expect(await screen.findByText('DeepSeek V4 Pro')).toBeInTheDocument();
    expect(screen.getAllByRole('link', { name: '官网价格' })[0]).toHaveAttribute('href','https://api-docs.deepseek.com/quick_start/pricing/');
    fireEvent.change(screen.getByRole('combobox',{name:'价格来源'}),{target:{value:'opencode'}});
    expect(screen.queryByText('DeepSeek V4 Pro')).not.toBeInTheDocument();
    expect(screen.getByText('DeepSeek Free')).toBeInTheDocument();
    expect(screen.getAllByText('$0')).toHaveLength(3);
    fireEvent.change(screen.getByRole('searchbox',{name:'查找模型'}),{target:{value:'unlisted'}});
    expect(screen.getByText('没有匹配的价格。')).toBeInTheDocument();
  });
  beforeEach(async () => {
    vi.clearAllMocks();
    await i18n.changeLanguage('zh-CN');
    api.updateWorkspaceSettings.mockResolvedValue(undefined);
    api.getSourceSettings.mockResolvedValue([
      { agent: 'codex', path: 'C:/Users/test/.codex', available: true },
      { agent: 'claude', path: 'C:/Users/test/.claude', available: false },
      { agent: 'opencode', path: 'C:/Users/test/.local/share/opencode', available: false },
      { agent: 'dsh', path: 'C:/Users/test/.dsh', customPath: 'D:/Profiles/dsh', available: true },
    ]);
    api.getBootstrapStatus.mockResolvedValue({
      appVersion: '2.1.0', platform: 'windows',
      dataDirectory: 'C:\\Users\\tester\\AppData\\Local\\Chronolume\\v2',
      databasePath: 'C:\\Users\\tester\\AppData\\Local\\Chronolume\\v2\\chronolume-v2.sqlite3',
      schemaVersion: 1, databaseSizeBytes: 1024,
    });
    api.saveModelPrice.mockResolvedValue({ prices: [], reprice: {} });
    api.deleteModelPrice.mockResolvedValue({ prices: [], reprice: {} });
    api.restoreBuiltinPrice.mockResolvedValue({ prices: [], reprice: {} });
    api.startSync.mockResolvedValue({ phase: 'completed' });
    api.getSyncStatus.mockResolvedValue({ phase: 'idle' });
    api.clearAnalysis.mockResolvedValue(undefined);
  });

  it('lists, edits, and opens workspace analytics', async () => {
    api.getWorkspaces.mockResolvedValue({
      page: 0, pageSize: 25, total: 1,
      items: [{
        id: 'workspace-1', label: 'Alpha', normalizedPath: 'D:/work/alpha', ignored: false,
        sessionCount: 4, totalTokens: 12_000, estimatedCostMicrousd: 750_000,
        unpricedEventCount: 0, activeMs: 60_000, activeDays: 2, lastActivityAtMs: 1_788_000_000_000,
      }],
    });
    const open = vi.fn();
    renderPage(<ProjectsPage filters={filters} onOpenWorkspace={open} />);

    expect(await screen.findByText('Alpha')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: '打开工作区统计' }));
    expect(open).toHaveBeenCalledWith('workspace-1');
    fireEvent.click(screen.getByRole('button', { name: '编辑工作区' }));
    fireEvent.change(screen.getByLabelText('别名'), { target: { value: 'Alpha alias' } });
    fireEvent.click(screen.getByRole('button', { name: '保存' }));
    await waitFor(() => expect(api.updateWorkspaceSettings).toHaveBeenCalledWith('workspace-1', 'Alpha alias', false));
  });

  it('opens structured session details and server-paged token events', async () => {
    const session = {
      id: 'session-1', title: '2026-07-10 · abc123', workspaceId: 'workspace-1', workspaceLabel: 'Alpha',
      startedAtMs: 1_788_000_000_000, endedAtMs: 1_788_000_060_000,
      activeMs: 60_000, activeMethod: 'lifecycle', activeIsEstimate: false,
      modelProvider: 'openai', latestModel: 'gpt-5.6-sol', totalTokens: 120,
      inputTokens: 90, freshInputTokens: 70, cachedInputTokens: 20, outputTokens: 30,
      averageFirstTokenLatencyMs: 1250, firstTokenLatencySampleCount: 2,
      reasoningTokens: 10, tokensPerSecond: 0.5, estimatedCostMicrousd: 5_000, unpricedEventCount: 0,
      archived: false, integrityStatus: 'complete',
    };
    api.getSessions.mockResolvedValue({ page: 0, pageSize: 30, total: 1, items: [session] });
    api.getSessionDetail.mockResolvedValue({
      session,
      parsing: { sourceKind: 'session', sourceStatus: 'ready', parserVersion: 3, warningCount: 0 },
      modelSegments: [{
        segmentIndex: 0, startedAtMs: session.startedAtMs, provider: 'openai', model: 'gpt-5.6-sol',
        inputTokens: 90, cachedInputTokens: 20, outputTokens: 30, reasoningTokens: 10,
        estimatedCostMicrousd: 5_000, unpricedEventCount: 0,
      }],
      activitySegments: [{
        segmentIndex: 0, startedAtMs: session.startedAtMs, endedAtMs: session.endedAtMs,
        activeMs: 60_000, method: 'lifecycle', isEstimate: false,
      }],
      tools: [{ toolName: 'apply_patch', category: 'edit', operationKind: 'mutating', callCount: 2, sessionCount: 1 }],
      recentUsageEvents: [], retainedEventCount: 1,
    });
    api.getUsageEvents.mockResolvedValue({
      page: 0, pageSize: 100, total: 1,
      items: [{
        id: 1, occurredAtMs: session.startedAtMs, provider: 'openai', model: 'gpt-5.6-sol',
        inputTokens: 90, cachedInputTokens: 20, outputTokens: 30, reasoningTokens: 10,
        totalTokens: 120, estimatedCostMicrousd: 5_000, integrityStatus: 'complete',
      }],
    });
    renderPage(<SessionsPage filters={filters} />);

    expect(await screen.findByText('0.5 token/s')).toBeInTheDocument();
    expect(screen.getByText('1.25 s')).toHaveAttribute('title', expect.stringContaining('有效轮次：2'));
    expect(screen.getByRole('columnheader', { name: 'TPS' })).toHaveAttribute('title', expect.stringContaining('含推理'));
    fireEvent.change(screen.getByRole('combobox', { name: '排序' }), { target: { value: 'tps' } });
    await waitFor(() => expect(api.getSessions).toHaveBeenLastCalledWith(expect.objectContaining({ sort: 'tps', descending: true })));
    fireEvent.click(screen.getByRole('button', { name: '降序' }));
    await waitFor(() => expect(api.getSessions).toHaveBeenLastCalledWith(expect.objectContaining({ sort: 'tps', descending: false })));
    fireEvent.change(screen.getByRole('combobox', { name: '排序' }), { target: { value: 'ttft' } });
    await waitFor(() => expect(api.getSessions).toHaveBeenLastCalledWith(expect.objectContaining({ sort: 'ttft', descending: false })));
    fireEvent.click(await screen.findByRole('button', { name: '查看会话详情' }));
    expect(await screen.findByText('仅显示结构化统计；不读取或显示对话正文。')).toBeInTheDocument();
    expect(screen.getByText('apply_patch · edit')).toBeInTheDocument();
    expect(within(screen.getByLabelText('会话结构化详情')).getByText(/生命周期/)).toBeInTheDocument();
    expect(within(screen.getByLabelText('会话结构化详情')).getByText('0.5 token/s')).toBeInTheDocument();
    expect(within(screen.getByLabelText('会话结构化详情')).getByText('1.25 s')).toBeInTheDocument();
    expect(api.getUsageEvents).toHaveBeenCalledWith('session-1', 0);
  });

  it('shows unpriced usage and supports price add, edit, delete, and restore', async () => {
    api.getModels.mockResolvedValue({
      page: 0, pageSize: 30, total: 1,
      items: [{
        model: 'gpt-5.6-custom', sessionCount: 1,
        inputTokens: 100, freshInputTokens: 80, cachedInputTokens: 20, outputTokens: 10,
        reasoningTokens: 5, totalTokens: 115, cacheHitRate: .2, averageTokensPerSecond: 12.34,
        averageFirstTokenLatencyMs: 0, firstTokenLatencySampleCount: 1,
        estimatedCostMicrousd: undefined, unpricedEventCount: 1,
      }],
    });
    api.listModelPrices.mockResolvedValue([{
      provider: 'openai', pricingId: 'gpt-5.6-sol', displayName: 'GPT-5.6 Sol',
      inputPerMillionUsd: '1.25', outputPerMillionUsd: '10', cacheReadPerMillionUsd: '0.125',
      isBuiltin: true, isOverridden: true, isDeleted: false, revision: 2,
    }]);
    renderPage(<ModelsPage filters={filters} />);

    expect((await screen.findAllByText('未定价')).length).toBeGreaterThan(0);
    expect(api.getModels).toHaveBeenCalledWith(expect.objectContaining({ sort: 'name', descending: true }));
    expect(screen.getByRole('columnheader', { name: '平均每百万 Token 成本' })).toBeInTheDocument();
    expect(screen.getByText('12.3 token/s')).toBeInTheDocument();
    expect(screen.getByText('0 ms')).toHaveAttribute('title', expect.stringContaining('有效轮次：1'));
    expect(screen.getByRole('columnheader', { name: '平均 TPS' })).toHaveAttribute('title', expect.stringContaining('按时长加权'));
    fireEvent.change(screen.getByRole('combobox', { name: '排序' }), { target: { value: 'tps' } });
    await waitFor(() => expect(api.getModels).toHaveBeenLastCalledWith(expect.objectContaining({ sort: 'tps', descending: true })));
    fireEvent.click(screen.getByRole('button', { name: '降序' }));
    await waitFor(() => expect(api.getModels).toHaveBeenLastCalledWith(expect.objectContaining({ sort: 'tps', descending: false })));
    fireEvent.change(screen.getByRole('combobox', { name: '排序' }), { target: { value: 'ttft' } });
    await waitFor(() => expect(api.getModels).toHaveBeenLastCalledWith(expect.objectContaining({ sort: 'ttft', descending: false })));
    expect(screen.queryByText('$0.00')).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('tab', { name: '价格表' }));
    expect(await screen.findByText('GPT-5.6 Sol')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: '编辑价格' }));
    fireEvent.change(screen.getByLabelText('输出'), { target: { value: '11' } });
    fireEvent.click(screen.getByRole('button', { name: '保存并重算' }));
    await waitFor(() => expect(api.saveModelPrice).toHaveBeenCalledWith(expect.objectContaining({ pricingId: 'gpt-5.6-sol', outputPerMillionUsd: '11' })));

    fireEvent.click(screen.getByRole('button', { name: '删除价格' }));
    fireEvent.click(screen.getByRole('button', { name: '恢复内置价格' }));
    await waitFor(() => {
      expect(api.deleteModelPrice).toHaveBeenCalledWith('openai', 'gpt-5.6-sol');
      expect(api.restoreBuiltinPrice).toHaveBeenCalledWith('openai', 'gpt-5.6-sol');
    });

    fireEvent.click(screen.getByRole('button', { name: '添加价格' }));
    fireEvent.change(screen.getByLabelText('计价 ID'), { target: { value: 'gpt-custom' } });
    fireEvent.change(screen.getByLabelText('显示名称'), { target: { value: 'Custom' } });
    fireEvent.change(screen.getByLabelText('输入'), { target: { value: '1' } });
    fireEvent.change(screen.getByLabelText('缓存读取'), { target: { value: '.1' } });
    fireEvent.change(screen.getByLabelText('输出'), { target: { value: '8' } });
    fireEvent.click(screen.getByRole('button', { name: '保存并重算' }));
    await waitFor(() => expect(api.saveModelPrice).toHaveBeenCalledWith(expect.objectContaining({ pricingId: 'gpt-custom', cacheWritePerMillionUsd: undefined })));
  });

  it('renders heatmap/tool analytics and exposes repair index', async () => {
    api.getHeatmap.mockResolvedValue({
      metric: 'sessions', span: 'year', startDate: '2026-07-09', endDate: '2026-07-10', maxValue: 2,
      points: [{ date: '2026-07-10', value: 2, sessionCount: 2, totalTokens: 500, activeMs: 60_000 }],
    });
    api.getTools.mockResolvedValue({
      totalCalls: 3, uniqueTools: 2,
      topTools: [{ toolName: 'apply_patch', category: 'edit', operationKind: 'mutating', callCount: 2, sessionCount: 1 }],
      categories: [{ category: 'edit', callCount: 2 }], trend: [{ date: '2026-07-10', callCount: 3 }],
    });
    api.getDiagnostics.mockResolvedValue({
      databaseSizeBytes: 1024, databaseIntegrityOk: true, schemaVersion: 1, parserVersion: 3,
      indexedSessions: 1, retainedUsageEvents: 1, retainedToolEvents: 1, sources: [], recentRuns: [], generatedAtMs: Date.now(),
    });

    const { unmount } = renderPage(<ActivityPage filters={filters} />);
    expect(await screen.findByText('apply_patch')).toBeInTheDocument();
    expect(screen.getByText('工具调用', { selector: '.stat-card span' }).parentElement).toHaveTextContent('3');
    unmount();

    renderPage(<DataPage />);
    const repair = await screen.findByRole('button', { name: '修复索引' });
    await waitFor(() => expect(repair).toBeEnabled());
    fireEvent.click(repair);
    await waitFor(() => expect(api.startSync).toHaveBeenCalledWith('repair'));
  });

  it('keeps maintenance disabled while indexing and never reports missing diagnostics as an error', async () => {
    api.getDiagnostics.mockImplementation(() => new Promise(() => undefined));
    api.getSyncStatus.mockResolvedValue({ phase: 'importing' });
    renderPage(<DataPage />);
    expect(screen.queryByText('异常')).not.toBeInTheDocument();
    expect(await screen.findByText('索引正在运行。请等待完成，或在上方取消后再执行维护操作。')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '清空分析库' })).toBeDisabled();
    expect(screen.getByRole('button', { name: '重新索引' })).toBeDisabled();
  });

  it('requires confirmation before clearing and preserves a retryable failure in the dialog', async () => {
    api.getDiagnostics.mockResolvedValue({ sources: [], recentRuns: [], databaseIntegrityOk: true, databaseSizeBytes: 0, indexedSessions: 0, retainedUsageEvents: 0, retainedToolEvents: 0 });
    api.clearAnalysis.mockRejectedValueOnce({ code: 'database_error', message: '分析数据库操作失败' });
    renderPage(<DataPage />);
    const clear = screen.getByRole('button', { name: '清空分析库' });
    await waitFor(() => expect(clear).toBeEnabled());
    fireEvent.click(clear);
    const dialog = screen.getByRole('dialog', { name: '清空分析库' });
    expect(api.clearAnalysis).not.toHaveBeenCalled();
    fireEvent.click(within(dialog).getByRole('button', { name: '确认清空' }));
    expect(await within(dialog).findByRole('alert')).toHaveTextContent('分析数据库操作失败');
    expect(dialog).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole('button', { name: '确认清空' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
    expect(api.clearAnalysis).toHaveBeenCalledTimes(2);
    expect(screen.getByRole('status')).toHaveTextContent('分析库已清空');
  });

  it('renders the backend-provided analytics directory for Windows and macOS', async () => {
    api.getDiagnostics.mockResolvedValue({
      databaseSizeBytes: 1024, databaseIntegrityOk: true, schemaVersion: 1, parserVersion: 3,
      indexedSessions: 1, retainedUsageEvents: 1, retainedToolEvents: 1, sources: [], recentRuns: [], generatedAtMs: Date.now(),
    });
    const { unmount } = renderPage(<DataPage />);
    expect(await screen.findByText('C:\\Users\\tester\\AppData\\Local\\Chronolume\\v2')).toBeInTheDocument();
    unmount();

    api.getBootstrapStatus.mockResolvedValue({
      appVersion: '2.1.0', platform: 'macos',
      dataDirectory: '/Users/tester/Library/Application Support/Chronolume/v2',
      databasePath: '/Users/tester/Library/Application Support/Chronolume/v2/chronolume-v2.sqlite3',
      schemaVersion: 1, databaseSizeBytes: 1024,
    });
    renderPage(<DataPage />);
    expect(await screen.findByText('/Users/tester/Library/Application Support/Chronolume/v2')).toBeInTheDocument();
    expect(screen.queryByText(/LOCALAPPDATA/)).not.toBeInTheDocument();
  });

  it('configures one agent directory while preserving the other source settings and syncing', async () => {
    api.getDiagnostics.mockResolvedValue({
      databaseSizeBytes: 1024, databaseIntegrityOk: true, schemaVersion: 5, parserVersion: 11,
      indexedSessions: 1, retainedUsageEvents: 1, retainedToolEvents: 1, sources: [], recentRuns: [], generatedAtMs: Date.now(),
    });
    api.saveSourceSettings.mockResolvedValue([
      { agent: 'codex', path: 'C:/Users/test/.codex', available: true },
      { agent: 'claude', path: 'D:/Profiles/claude', customPath: 'D:/Profiles/claude', available: true },
      { agent: 'opencode', path: 'C:/Users/test/.local/share/opencode', available: false },
      { agent: 'dsh', path: 'C:/Users/test/.dsh', customPath: 'D:/Profiles/dsh', available: true },
    ]);
    renderPage(<DataPage />);
    expect(await screen.findByText('C:/Users/test/.claude')).toBeInTheDocument();
    fireEvent.click(screen.getAllByRole('button', { name: '配置目录' })[1]);
    fireEvent.change(screen.getByLabelText('Claude Code · 数据目录'), { target: { value: 'D:/Profiles/claude' } });
    fireEvent.click(screen.getByRole('button', { name: '保存并同步' }));
    await waitFor(() => expect(api.saveSourceSettings).toHaveBeenCalledWith({ codex: null, claude: 'D:/Profiles/claude', opencode: null, dsh: 'D:/Profiles/dsh' }));
    expect(api.startSync).toHaveBeenCalledWith('incremental');
    await waitFor(() => expect(screen.queryByLabelText('Claude Code · 数据目录')).not.toBeInTheDocument());
  });
});
