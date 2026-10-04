import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import i18n from '../i18n';
import { ExportControls } from './ExportControls';
import type { UsageFilters } from '../types';

const native = vi.hoisted(() => ({ enabled: true, save: vi.fn(), exportData: vi.fn(), writeChartPng: vi.fn() }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ save: native.save }));
vi.mock('../api', () => ({ isTauriRuntime: () => native.enabled, exportData: native.exportData, writeChartPng: native.writeChartPng }));
const filters: UsageFilters = { range: { preset: 'last30_days', liveEnd: false }, archived: 'all' };

describe('ExportControls', () => {
  beforeEach(async () => {
    vi.resetAllMocks();
    native.enabled = true;
    native.save.mockResolvedValue('D:/exports/usage.csv');
    native.exportData.mockResolvedValue({});
    await i18n.changeLanguage('zh-CN');
  });
  const renderControls = () => render(<ExportControls scope="sessions" filters={filters} allowPng={false} />);

  it('reports a serialized native error and retries with the same scope and filters', async () => {
    native.exportData.mockRejectedValueOnce({ code: 'filesystem_error', message: '本地文件操作失败' });
    renderControls();
    fireEvent.click(screen.getByRole('button', { name: '导出' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('本地文件操作失败');
    fireEvent.click(screen.getByRole('button', { name: '导出' }));
    expect(await screen.findByRole('status')).toHaveTextContent('D:/exports/usage.csv');
    expect(native.exportData).toHaveBeenLastCalledWith({ format: 'csv', scope: 'sessions', privacy: 'anonymous', filters }, 'D:/exports/usage.csv');
  });

  it('handles cancellation and native save-dialog failure without reporting a successful export', async () => {
    native.save.mockResolvedValueOnce(null).mockRejectedValueOnce(new Error('dialog unavailable'));
    renderControls();
    fireEvent.click(screen.getByRole('button', { name: '导出' }));
    await waitFor(() => expect(screen.getByRole('button', { name: '导出' })).toBeEnabled());
    expect(native.exportData).not.toHaveBeenCalled();
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: '导出' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('dialog unavailable');
  });

  it('disables export in browser preview and prevents duplicate requests while the dialog is open', async () => {
    native.enabled = false;
    const { unmount } = renderControls();
    expect(screen.getByRole('button', { name: '导出' })).toBeDisabled();
    unmount();
    native.enabled = true;
    native.save.mockImplementation(() => new Promise(() => undefined));
    renderControls();
    fireEvent.click(screen.getByRole('button', { name: '导出' }));
    expect(screen.getByRole('button', { name: '正在导出…' })).toBeDisabled();
    expect(screen.getByRole('combobox', { name: '导出格式' })).toBeDisabled();
    expect(native.save).toHaveBeenCalledTimes(1);
  });
});
