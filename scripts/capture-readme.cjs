// Run with Playwright CLI against the local Vite server; see docs/readme-preview.md.
// This isolated browser fixture never reads Codex files or calls the native backend.
async (page) => {
  const errors = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.clock.setFixedTime(new Date('2026-10-04T16:00:00Z'));
  await page.addInitScript(() => {
    localStorage.setItem('chronolume.language', 'zh-CN');
    localStorage.setItem('chronolume.theme', 'light');
    localStorage.setItem('chronolume.font-scale', '1');
    localStorage.removeItem('chronolume.trend-series.v1');
    localStorage.removeItem('codex-usage.trend-series.v1');
    const now = Date.now();
    const day = 86_400_000;
    const catalog = [
      { model: 'gpt-5.5', weight: .52, cacheRate: .87, outputRate: .065, inputPrice: 5, cachePrice: .5, outputPrice: 30 },
      { model: 'gpt-5.4', weight: .31, cacheRate: .82, outputRate: .05, inputPrice: 2.5, cachePrice: .25, outputPrice: 15 },
      { model: 'gpt-5.4-mini', weight: .17, cacheRate: .73, outputRate: .04, inputPrice: .75, cachePrice: .075, outputPrice: 4.5 },
    ];
    const daily = [320, 510, 430, 0, 0, 620, 780, 540, 910, 670, 280, 0, 580, 840, 1120, 730, 890, 410, 0, 690, 960, 810, 1040, 760, 450, 0, 720, 980, 1250, 840];
    const byModel = catalog.map((model) => daily.map((value, i) => {
      const inputTokens = Math.round(value * 2_100 * model.weight);
      const cachedInputTokens = Math.round(inputTokens * model.cacheRate);
      const outputTokens = Math.round(inputTokens * model.outputRate);
      const activeMs = Math.round(value * 21_000 * model.weight);
      return {
        key: String(i), timestampMs: now - (29 - i) * day,
        inputTokens, cachedInputTokens, freshInputTokens: inputTokens - cachedInputTokens,
        outputTokens, reasoningTokens: Math.round(outputTokens * .38),
        totalTokens: inputTokens + outputTokens,
        estimatedCostMicrousd: Math.round((inputTokens - cachedInputTokens) * model.inputPrice + cachedInputTokens * model.cachePrice + outputTokens * model.outputPrice),
        unpricedEventCount: 0, sessionCount: value === 0 ? 0 : Math.max(1, Math.round(value / 190 * model.weight)), activeMs,
      };
    }));
    const sum = (rows, key) => rows.reduce((total, row) => total + row[key], 0);
    const numericKeys = ['inputTokens', 'cachedInputTokens', 'freshInputTokens', 'outputTokens', 'reasoningTokens', 'totalTokens', 'estimatedCostMicrousd', 'unpricedEventCount', 'sessionCount', 'activeMs'];
    const trend = daily.map((_, i) => ({ key: String(i), timestampMs: byModel[0][i].timestampMs, ...Object.fromEntries(numericKeys.map((key) => [key, sum(byModel.map((rows) => rows[i]), key)])) }));
    const totals = Object.fromEntries(numericKeys.map((key) => [key, sum(trend, key)]));
    const activeDays = trend.filter((point) => point.totalTokens > 0).length;
    const peak = trend.reduce((largest, point) => point.totalTokens > largest.totalTokens ? point : largest);
    let streak = 0;
    let longestActiveStreakDays = 0;
    for (const point of trend) {
      streak = point.totalTokens > 0 ? streak + 1 : 0;
      longestActiveStreakDays = Math.max(longestActiveStreakDays, streak);
    }
    const date = (timestamp) => new Date(timestamp).toISOString().slice(0, 10);
    const hero = {
      ...totals, realTotalTokens: totals.totalTokens, cacheHitRate: totals.cachedInputTokens / totals.inputTokens,
      activeDays, averageTokensPerDay: totals.totalTokens / 30,
      averageCostMicrousdPerDay: Math.round(totals.estimatedCostMicrousd / 30),
      averageSessionsPerDay: totals.sessionCount / 30, averageActiveMsPerDay: totals.activeMs / 30,
      peakDay: date(peak.timestampMs), peakDayTokens: peak.totalTokens, longestActiveStreakDays,
    };
    const models = catalog.map((model, i) => {
      const rows = byModel[i];
      const aggregate = Object.fromEntries(numericKeys.map((key) => [key, sum(rows, key)]));
      return {
        ...aggregate, model: model.model, pricingModelId: model.model,
        cacheHitRate: aggregate.cachedInputTokens / aggregate.inputTokens,
        averageTokensPerSecond: aggregate.outputTokens / (aggregate.activeMs / 1_000),
        averageCostMicrousdPerMillionTokens: Math.round(aggregate.estimatedCostMicrousd / aggregate.totalTokens * 1_000_000),
        lastUsedAtMs: now - i * 3_600_000,
      };
    });
    const workspaceSeeds = [
      ['demo-chronolume', 'Chronolume', 'D:/Projects/Chronolume', .42],
      ['demo-research', 'Research Notes', 'D:/Projects/research-notes', .27],
      ['demo-agent', 'Agent Playground', 'D:/Projects/agent-playground', .19],
      ['demo-web', 'Web Toolkit', 'D:/Projects/web-toolkit', .12],
    ];
    const distribute = (key) => {
      const values = workspaceSeeds.map((item) => Math.floor(totals[key] * item[3]));
      values[0] += totals[key] - sum(values.map((value) => ({ value })), 'value');
      return values;
    };
    const workspaces = workspaceSeeds.map(([id, label, normalizedPath], i) => ({
      id, label, normalizedPath, ignored: false,
      sessionCount: distribute('sessionCount')[i], totalTokens: distribute('totalTokens')[i],
      estimatedCostMicrousd: distribute('estimatedCostMicrousd')[i], unpricedEventCount: 0,
      activeMs: distribute('activeMs')[i], activeDays: activeDays - i * 2, lastActivityAtMs: now - i * day,
    }));
    const sessions = Array.from({ length: 5 }, (_, i) => {
      const workspace = workspaces[i % workspaces.length];
      const model = catalog[i % catalog.length];
      const inputTokens = 720_000 - i * 81_000;
      const cachedInputTokens = Math.round(inputTokens * model.cacheRate);
      const outputTokens = Math.round(inputTokens * model.outputRate);
      const activeMs = (75 - i * 8) * 60_000;
      return {
        id: `demo-session-${i}`, title: `${date(now - i * day)} · ${['a72c41', 'f30b68', '9d2e15', 'c61a89', '7b40f2'][i]}`,
        workspaceId: workspace.id, workspaceLabel: workspace.label,
        startedAtMs: now - i * day - activeMs, endedAtMs: now - i * day,
        activeMs, activeMethod: 'lifecycle', activeIsEstimate: false,
        modelProvider: 'openai', latestModel: model.model,
        inputTokens, cachedInputTokens, freshInputTokens: inputTokens - cachedInputTokens,
        outputTokens, reasoningTokens: Math.round(outputTokens * .38), totalTokens: inputTokens + outputTokens,
        tokensPerSecond: outputTokens / (activeMs / 1_000),
        estimatedCostMicrousd: Math.round((inputTokens - cachedInputTokens) * model.inputPrice + cachedInputTokens * model.cachePrice + outputTokens * model.outputPrice),
        unpricedEventCount: 0, archived: i === 4, integrityStatus: 'complete',
      };
    });
    const tools = {
      totalCalls: 4830, uniqueTools: 6,
      categories: [{ category: 'read', callCount: 1580 }, { category: 'search', callCount: 1020 }, { category: 'edit', callCount: 940 }, { category: 'execute', callCount: 820 }, { category: 'write', callCount: 320 }, { category: 'other', callCount: 150 }],
      topTools: [
        { toolName: 'read_file', category: 'read', operationKind: 'read_only', callCount: 1580, sessionCount: 54 },
        { toolName: 'search_files', category: 'search', operationKind: 'read_only', callCount: 1020, sessionCount: 46 },
        { toolName: 'apply_patch', category: 'edit', operationKind: 'mutating', callCount: 940, sessionCount: 38 },
        { toolName: 'exec_command', category: 'execute', operationKind: 'unknown', callCount: 820, sessionCount: 44 },
        { toolName: 'write_file', category: 'write', operationKind: 'mutating', callCount: 320, sessionCount: 22 },
        { toolName: 'list_resources', category: 'other', operationKind: 'read_only', callCount: 150, sessionCount: 16 },
      ],
      trend: trend.map((point, i) => ({ date: date(point.timestampMs), callCount: Math.round(daily[i] / sum(daily.map((value) => ({ value })), 'value') * 4830) })),
    };
    // Keep category, top-tool and trend totals internally consistent.
    tools.trend[0].callCount += tools.totalCalls - sum(tools.trend, 'callCount');
    const heatmapPoints = Array.from({ length: 365 }, (_, i) => {
      const point = i >= 335 ? trend[i - 335] : undefined;
      const timestamp = now - (364 - i) * day;
      const value = point?.totalTokens ?? (i % 7 >= 5 ? 0 : Math.round((220 + (i * 37 % 630)) * 2_100));
      return { date: date(timestamp), value, totalTokens: value, sessionCount: point?.sessionCount ?? (value > 0 ? 4 : 0), activeMs: point?.activeMs ?? Math.round(value * 9) };
    });
    const callbacks = new Map();
    const syncStatus = { phase: 'completed', filesTotal: totals.sessionCount, filesCompleted: totals.sessionCount, bytesTotal: 48_600_000, bytesRead: 48_600_000, recordsWritten: 36_240, recordsSkipped: 0, parseFailures: 0, fileErrors: 0, speedBytesPerSecond: 0, updatedAtMs: now, lastCompletedAtMs: now, cancelRequested: false };
    const list = (items, query) => ({ items, page: query.page, pageSize: query.pageSize, total: items.length });
    window.__CHRONOLUME_README_DEMO__ = true;
    window.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main' } },
      transformCallback(callback) { const id = callbacks.size + 1; callbacks.set(id, callback); return id; },
      unregisterCallback(id) { callbacks.delete(id); },
      async invoke(command, args) {
        if (command.startsWith('plugin:event|')) return 1;
        if (command === 'plugin:window|set_theme') return;
        if (command === 'get_bootstrap_status') return { appVersion: '2.1.9 · DEMO', platform: 'windows', dataDirectory: 'D:/Demo/Chronolume', databasePath: 'D:/Demo/Chronolume/demo.sqlite3', databaseSizeBytes: 9_830_400, schemaVersion: 1 };
        if (command === 'get_app_preferences') return { idleGapMinutes: 30, visibleWorkspaceIds: workspaces.map((item) => item.id) };
        if (command === 'get_workspace_catalog') return workspaces.map((item) => ({ value: item.id, label: item.label, normalizedPath: item.normalizedPath }));
        if (command === 'get_sync_status') return syncStatus;
        if (command === 'get_dashboard') return {
          resolvedRange: { startMs: trend[0].timestampMs, endMs: now, startLocalDate: date(trend[0].timestampMs), endLocalDate: date(now), calendarDays: 30, granularity: 'day' },
          hero, trend, dataState: 'complete', generatedAtMs: now,
          filterOptions: { workspaces: workspaces.map((item) => ({ value: item.id, label: item.label })), providers: [{ value: 'openai', label: 'OpenAI' }], models: models.map((item) => ({ value: item.model, label: item.model })) },
        };
        if (command === 'get_heatmap') return { metric: args.query.metric, span: args.query.span, startDate: heatmapPoints[0].date, endDate: date(now), maxValue: Math.max(...heatmapPoints.map((point) => point.value)), points: heatmapPoints };
        if (command === 'get_workspaces') return list(workspaces, args.query);
        if (command === 'get_sessions') return list(sessions, args.query);
        if (command === 'get_models') return list(models, args.query);
        if (command === 'get_tools') return tools;
        if (command === 'list_model_prices') return catalog.map((item) => ({ provider: 'openai', pricingId: item.model, displayName: item.model, inputPerMillionUsd: String(item.inputPrice), outputPerMillionUsd: String(item.outputPrice), cacheReadPerMillionUsd: String(item.cachePrice), isBuiltin: true, isDeleted: false, isOverridden: false, revision: 2026092901 }));
        throw new Error(`README fixture does not implement ${command}. No native operation was performed.`);
      },
    };
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
  });
  await page.goto('http://127.0.0.1:1420/');
  await page.locator('.recharts-surface').waitFor();
  const screenshots = [];
  const capture = async (filename) => {
    await page.evaluate(() => window.scrollTo(0, 0));
    const height = await page.locator('.app-footer').evaluate((footer) => Math.ceil(footer.getBoundingClientRect().bottom + 24));
    // Fit the actual content so the sticky navigation also spans the screenshot.
    await page.setViewportSize({ width: 1440, height });
    await page.evaluate(async () => {
      await document.fonts.ready;
      document.activeElement?.blur();
    });
    await page.mouse.move(0, 0);
    await page.screenshot({ path: `docs/images/${filename}`, fullPage: true, animations: 'disabled' });
    screenshots.push(filename);
  };
  await capture('chronolume-dashboard.png');
  for (const [label, filename] of [
    ['项目', 'chronolume-projects.png'],
    ['会话', 'chronolume-sessions.png'],
    ['模型与成本', 'chronolume-models.png'],
    ['工具与活动', 'chronolume-activity.png'],
  ]) {
    await page.locator('.sidebar').getByRole('button', { name: label, exact: true }).click();
    await page.locator('.feature-page').waitFor();
    await page.locator('.data-table tbody tr, .stat-card strong').first().waitFor();
    await capture(filename);
  }
  await page.locator('.sidebar').getByRole('button', { name: '模型与成本', exact: true }).click();
  await page.getByRole('tab', { name: '价格表', exact: true }).click();
  await page.getByRole('region', { name: '价格表', exact: true }).locator('tbody tr').first().waitFor();
  await capture('chronolume-prices.png');
  await page.locator('.sidebar').getByRole('button', { name: '设置', exact: true }).click();
  await page.getByRole('button', { name: '暗色', exact: true }).click();
  await page.locator('.sidebar').getByRole('button', { name: '总览', exact: true }).click();
  await page.locator('.recharts-surface').waitFor();
  await capture('chronolume-dashboard-dark.png');
  if (errors.length) throw new Error(errors.join('\n'));
  return { screenshots, data: 'deterministic synthetic data only', nativeOperations: 0, errors };
}
