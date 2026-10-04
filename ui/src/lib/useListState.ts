import { useEffect, useState } from 'react';

export function useListState(context: unknown, defaultSort: string) {
  const contextKey = JSON.stringify(context);
  const [search, setSearch] = useState('');
  const [querySearch, setQuerySearch] = useState('');
  const [sort, setSort] = useState(defaultSort);
  const [descending, setDescending] = useState(true);
  const [position, setPosition] = useState({ contextKey, page: 0 });
  const page = position.contextKey === contextKey && querySearch === search ? position.page : 0;
  const setPage = (next: number) => setPosition({ contextKey, page: next });
  useEffect(() => {
    setPosition((current) => current.contextKey === contextKey ? current : { contextKey, page: 0 });
  }, [contextKey]);
  useEffect(() => {
    const timer = window.setTimeout(() => setQuerySearch(search), 200);
    return () => window.clearTimeout(timer);
  }, [search]);

  return {
    search, querySearch, sort, descending, page, setPage,
    onSearch: (value: string) => { setSearch(value); setPage(0); },
    onSort: (value: string) => { setSort(value); setPage(0); },
    onDescending: (value: boolean) => { setDescending(value); setPage(0); },
  };
}
