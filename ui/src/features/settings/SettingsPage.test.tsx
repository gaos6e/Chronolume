import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import i18n from '../../i18n';
import { SettingsPage } from './SettingsPage';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';

const api = vi.hoisted(() => ({ getAppPreferences: vi.fn(), saveAppPreferences: vi.fn() }));
vi.mock('../../api', () => ({ ...api, isTauriRuntime: () => true }));

describe('SettingsPage', () => {
  beforeEach(async () => {
    localStorage.clear();
    vi.resetAllMocks();
    api.getAppPreferences.mockResolvedValue({ idleGapMinutes: 30, visibleWorkspaceIds: ['workspace-a'] });
    api.saveAppPreferences.mockImplementation(async (value) => value);
    document.documentElement.dataset.theme = '';
    await i18n.changeLanguage('zh-CN');
  });

  it('validates the idle interval and reports save failures without losing the draft or workspace visibility', async () => {
    api.saveAppPreferences.mockRejectedValueOnce({ code: 'database_error', message: '分析数据库操作失败' });
    render(<QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}><SettingsPage /></QueryClientProvider>);
    const input = screen.getByRole('spinbutton', { name: '活跃时间空闲间隔' });
    await waitFor(() => expect(input).toBeEnabled());
    fireEvent.change(input, { target: { value: '0' } });
    expect(screen.getByRole('button', { name: '保存' })).toBeDisabled();
    expect(screen.getByRole('alert')).toHaveTextContent('请输入 1–240');
    expect(api.saveAppPreferences).not.toHaveBeenCalled();
    fireEvent.change(input, { target: { value: '45' } });
    fireEvent.click(screen.getByRole('button', { name: '保存' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('分析数据库操作失败');
    expect(input).toHaveValue(45);
    fireEvent.click(screen.getByRole('button', { name: '保存' }));
    expect(await screen.findByRole('status')).toHaveTextContent('偏好已保存');
    expect(api.saveAppPreferences).toHaveBeenLastCalledWith({ idleGapMinutes: 45, visibleWorkspaceIds: ['workspace-a'] });
  });

  it('persists language, theme, and font scale locally', async () => {
    render(<QueryClientProvider client={new QueryClient()}><SettingsPage /></QueryClientProvider>);
    fireEvent.change(screen.getByRole('combobox'), { target: { value: 'en' } });
    await waitFor(() => expect(localStorage.getItem('chronolume.language')).toBe('en'));
    expect(screen.getByText('Theme')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Light' }));
    expect(document.documentElement.dataset.theme).toBe('light');
    fireEvent.change(screen.getByRole('slider'), { target: { value: '1.2' } });
    expect(localStorage.getItem('chronolume.font-scale')).toBe('1.2');
  });
});
