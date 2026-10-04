import { act, renderHook, waitFor } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { useListState } from './useListState';

describe('paged list state', () => {
  it('resets pagination for changed filters, sorting, direction, and returning to a previous filter', () => {
    const { result, rerender } = renderHook(({ workspace }) => useListState({ workspace }, 'recent'), { initialProps: { workspace: 'a' } });
    act(() => result.current.setPage(3));
    rerender({ workspace: 'b' });
    expect(result.current.page).toBe(0);
    rerender({ workspace: 'a' });
    expect(result.current.page).toBe(0);
    act(() => result.current.setPage(2));
    act(() => result.current.onSort('tokens'));
    expect(result.current.page).toBe(0);
    act(() => result.current.setPage(2));
    act(() => result.current.onDescending(false));
    expect(result.current.page).toBe(0);
  });

  it('keeps typing responsive and coalesces search queries at the first page', async () => {
    const { result } = renderHook(() => useListState({}, 'recent'));
    act(() => result.current.setPage(3));
    act(() => result.current.onSearch('a'));
    act(() => result.current.onSearch('alpha'));
    expect(result.current.search).toBe('alpha');
    expect(result.current.querySearch).toBe('');
    expect(result.current.page).toBe(0);
    await waitFor(() => expect(result.current.querySearch).toBe('alpha'));
  });
});
