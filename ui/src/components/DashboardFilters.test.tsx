import { fireEvent, render, screen } from '@testing-library/react';
import { useState } from 'react';
import { describe, expect, it, vi } from 'vitest';
import '../i18n';
import { cascadeFilter, DashboardFilters } from './DashboardFilters';
import type { UsageFilters } from '../types';

const filters: UsageFilters = {
  range: { preset: 'last30_days', liveEnd: false },
  workspaceId: 'workspace-a',
  modelProvider: 'openai',
  model: 'gpt-5.6-codex',
  archived: 'all',
};

describe('cascadeFilter', () => {
  it.each([['DeepSeek Harness', 'dsh'], ['WorkBuddy', 'workbuddy'], ['Z Code', 'zcode'], ['Google Antigravity', 'antigravity']])('selects %s and clears incompatible workspace and model filters', (label, agent) => {
    const onChange = vi.fn();
    render(<DashboardFilters filters={filters} refreshing={false} onChange={onChange} onRefresh={vi.fn()} />);
    fireEvent.click(screen.getByRole('button', { name: label }));
    expect(onChange).toHaveBeenCalledWith({ ...filters, agent, workspaceId: undefined, modelProvider: undefined, model: undefined });
  });
  it('keeps a workspace when selecting its owner and shows only that agent’s workspaces', () => {
    const onChange = vi.fn();
    const options = { providers: [], models: [], vendors: [], workspaces: [
      { agent: 'claude', value: 'workspace-a', label: 'Claude Code-Gaos' },
      { agent: 'codex', value: 'workspace-b', label: 'Codex-Gaos' },
    ] };
    const { rerender } = render(<DashboardFilters filters={filters} options={options} refreshing={false} onChange={onChange} onRefresh={vi.fn()} />);
    fireEvent.click(screen.getByRole('button', { name: 'Claude Code' }));
    expect(onChange).toHaveBeenCalledWith({ ...filters, agent: 'claude', modelProvider: undefined, model: undefined });
    rerender(<DashboardFilters filters={{ ...filters, agent: 'claude' }} options={options} refreshing={false} onChange={onChange} onRefresh={vi.fn()} />);
    expect(screen.getByRole('option', { name: 'Claude Code-Gaos' })).toBeInTheDocument();
    expect(screen.queryByRole('option', { name: 'Codex-Gaos' })).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Codex' }));
    expect(onChange).toHaveBeenLastCalledWith({ ...filters, agent: 'codex', workspaceId: undefined, modelProvider: undefined, model: undefined });
  });
  it('clears provider and model when the workspace changes', () => {
    expect(cascadeFilter(filters, 'workspaceId', 'workspace-b')).toMatchObject({
      workspaceId: 'workspace-b',
      modelProvider: undefined,
      model: undefined,
    });
  });

  it('supports multiple model vendors without a separate API route control', () => {
    const onChange = vi.fn();
    function SessionFilters() {
      const [current, setCurrent] = useState(filters);
      return <DashboardFilters
        filters={current}
        options={{ workspaces: [], providers: [], models: [], vendors: ['unknown', 'anthropic', 'openai'] }}
        refreshing={false}
        onChange={(next) => { setCurrent(next); onChange(next); }}
        onRefresh={vi.fn()}
      />;
    }
    render(<SessionFilters />);
    fireEvent.click(screen.getByText('全部供应商'));
    expect(screen.getAllByRole('checkbox').map((checkbox) => checkbox.closest('label')?.textContent))
      .toEqual(['Anthropic', 'OpenAI', '未识别供应商']);
    fireEvent.click(screen.getByText('OpenAI').parentElement as HTMLLabelElement);
    expect(screen.getByRole('checkbox', { name: 'OpenAI' })).toBeChecked();
    fireEvent.click(screen.getByText('Anthropic'));
    expect(screen.getByText('已选 2 家供应商')).toBeInTheDocument();
    expect(screen.queryByRole('combobox', { name: '提供方' })).not.toBeInTheDocument();
    expect(screen.queryByRole('combobox', { name: '模型' })).not.toBeInTheDocument();
    expect(onChange).toHaveBeenLastCalledWith({
      ...filters, modelProvider: undefined, model: undefined, modelVendors: ['anthropic', 'openai'],
    });
    fireEvent.click(screen.getByRole('button', { name: '清除供应商筛选' }));
    expect(screen.getByText('全部供应商')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('checkbox', { name: 'OpenAI' }));
    fireEvent.click(screen.getByRole('checkbox', { name: 'OpenAI' }));
    expect(screen.getByText('全部供应商')).toBeInTheDocument();
    const trigger = screen.getByRole('button', { name: '模型供应商 全部供应商' });
    fireEvent.keyDown(screen.getByRole('checkbox', { name: 'OpenAI' }), { key: 'Escape' });
    expect(trigger).toHaveAttribute('aria-expanded', 'false');
    expect(trigger).toHaveFocus();
    fireEvent.click(trigger);
    expect(screen.getByRole('checkbox', { name: 'OpenAI' })).not.toBeChecked();
    fireEvent.pointerDown(document.body);
    expect(trigger).toHaveAttribute('aria-expanded', 'false');
  });

  it('offers every time preset and supports a live-ended custom range', () => {
    const onChange = vi.fn();
    render(<DashboardFilters filters={filters} refreshing={false} onChange={onChange} onRefresh={vi.fn()} />);
    expect(screen.getAllByRole('button', { pressed: false }).length).toBeGreaterThanOrEqual(7);
    fireEvent.click(screen.getByRole('button', { name: '自定义' }));
    const custom = onChange.mock.calls.at(-1)?.[0] as UsageFilters;
    expect(custom.range).toMatchObject({ preset: 'custom', liveEnd: false });

    render(<DashboardFilters filters={{ ...custom, range: { ...custom.range, liveEnd: true } }} refreshing={false} onChange={onChange} onRefresh={vi.fn()} />);
    expect(screen.getByLabelText('结束时间跟随现在')).toBeChecked();
    expect(screen.getByLabelText('结束')).toBeDisabled();
  });

  it('opens workspace management without applying the sentinel as a filter', () => {
    const onChange = vi.fn();
    const onManageWorkspaces = vi.fn();
    render(<DashboardFilters
      filters={{ ...filters, workspaceId: undefined }}
      options={{ workspaces: [], providers: [], models: [], vendors: [] }}
      refreshing={false}
      onChange={onChange}
      onRefresh={vi.fn()}
      onManageWorkspaces={onManageWorkspaces}
    />);

    fireEvent.change(screen.getByLabelText('工作区'), { target: { value: '__manage_workspaces__' } });
    expect(onManageWorkspaces).toHaveBeenCalledOnce();
    expect(onChange).not.toHaveBeenCalled();
  });

  it('keeps invalid custom dates out of queries and restores valid input', () => {
    const onChange = vi.fn();
    const custom = { ...filters, range: { preset: 'custom' as const, startMs: new Date('2026-01-01T12:00:32.123').getTime(), endMs: new Date('2026-01-02T12:00').getTime(), liveEnd: false } };
    render(<DashboardFilters filters={custom} refreshing={false} onChange={onChange} onRefresh={vi.fn()} />);
    fireEvent.change(screen.getByLabelText('结束'), { target: { value: '2025-12-31T12:00' } });
    expect(screen.getByRole('alert')).toHaveTextContent('统计仍使用上一次有效范围');
    expect(onChange).not.toHaveBeenCalled();
    fireEvent.change(screen.getByLabelText('结束'), { target: { value: '2026-01-03T12:00' } });
    expect(onChange).toHaveBeenLastCalledWith(expect.objectContaining({ range: expect.objectContaining({ startMs: custom.range.startMs, endMs: new Date('2026-01-03T12:00').getTime() }) }));
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });
});
