import { useState } from 'react';
import { keepPreviousData, useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { ChevronRight, FolderKanban, Pencil, Save } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { getWorkspaces, updateWorkspaceSettings } from '../../api';
import { PageControls } from '../../components/PageControls';
import { QueryStatus } from '../../components/QueryStatus';
import { ActionFeedback } from '../../components/ActionFeedback';
import { Dialog } from '../../components/Dialog';
import { useListState } from '../../lib/useListState';
import { invalidateUsageData } from '../../lib/queries';
import { formatCost, formatDuration, formatTokens } from '../../lib/format';
import type { UsageFilters } from '../../types';

export function ProjectsPage({
  filters,
  onOpenWorkspace,
  visibleWorkspaceIds = [],
  onManageWorkspaces = () => undefined,
}: {
  filters: UsageFilters;
  onOpenWorkspace: (workspaceId: string) => void;
  visibleWorkspaceIds?: string[];
  onManageWorkspaces?: () => void;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const list = useListState({ filters, visibleWorkspaceIds }, 'recent');
  const [editing, setEditing] = useState<{ id: string; alias: string; ignored: boolean }>();
  const request = { filters, workspaceIds: visibleWorkspaceIds, search: list.querySearch, sort: list.sort, descending: list.descending, page: list.page, pageSize: 25 };
  const workspaces = useQuery({
    queryKey: ['workspaces', request],
    queryFn: () => getWorkspaces(request),
    placeholderData: keepPreviousData,
  });
  const save = useMutation({
    mutationFn: () => editing
      ? updateWorkspaceSettings(editing.id, editing.alias || undefined, editing.ignored)
      : Promise.resolve(),
    onSuccess: () => {
      setEditing(undefined);
      invalidateUsageData(queryClient);
    },
  });

  return (
    <section className="feature-page">
      <PageControls
        {...list} onPage={list.setPage} loading={workspaces.isFetching}
        pageSize={25} total={workspaces.data?.total ?? 0}
        sortOptions={[
          { value: 'recent', label: t('最近活动') }, { value: 'tokens', label: 'Token' },
          { value: 'cost', label: t('成本') }, { value: 'active_time', label: t('活跃时间') },
          { value: 'sessions', label: t('会话数') }, { value: 'name', label: t('名称') },
        ]}
      />
      <QueryStatus
        loading={workspaces.isLoading && !workspaces.data}
        error={workspaces.error}
        refreshing={workspaces.isFetching && Boolean(workspaces.data)}
        onRetry={() => void workspaces.refetch()}
      />
      <ActionFeedback success={save.isSuccess && '工作区设置已保存。'} />
      {workspaces.data && (
      <div className="data-table-wrap" role="region" aria-label={t('工作区')} tabIndex={0} aria-busy={workspaces.isFetching}>
        <table className="data-table">
          <thead><tr><th>{t('工作区')}</th><th>{t('会话')}</th><th>Token</th><th>{t('成本')}</th><th>{t('活跃')}</th><th>{t('最近活动')}</th><th><span className="sr-only">{t('操作')}</span></th></tr></thead>
          <tbody>
            {workspaces.data?.items.map((workspace) => (
              <tr key={workspace.id}>
                <td><div className="primary-cell"><FolderKanban /><div><strong>{workspace.label}</strong><small>{workspace.normalizedPath}</small></div></div></td>
                <td>{workspace.sessionCount.toLocaleString()}</td>
                <td>{formatTokens(workspace.totalTokens)}</td>
                <td>{formatCost(workspace.estimatedCostMicrousd)}{workspace.unpricedEventCount > 0 && <small className="warning-copy"> {t('部分未定价')}</small>}</td>
                <td>{formatDuration(workspace.activeMs)}</td>
                <td>{workspace.lastActivityAtMs ? new Date(workspace.lastActivityAtMs).toLocaleString() : '—'}</td>
                <td><div className="row-actions"><button type="button" className="icon-button" aria-label={t('编辑工作区')} onClick={() => { save.reset(); setEditing({ id: workspace.id, alias: workspace.label, ignored: workspace.ignored }); }}><Pencil /></button><button type="button" className="icon-button" aria-label={t('打开工作区统计')} onClick={() => onOpenWorkspace(workspace.id)}><ChevronRight /></button></div></td>
              </tr>
            ))}
          </tbody>
        </table>
        {!workspaces.isLoading && workspaces.data?.items.length === 0 && <div className="table-empty workspace-empty"><span>{visibleWorkspaceIds.length === 0 ? t('尚未选择要显示的工作区。') : t('没有匹配的工作区。')}</span>{visibleWorkspaceIds.length === 0 && <button type="button" className="primary-button" onClick={onManageWorkspaces}>{t('选择工作区')}</button>}</div>}
      </div>
      )}
      {editing && (
        <Dialog className="editor-dialog" label={t('工作区设置')} busy={save.isPending} onClose={() => setEditing(undefined)}>
        <form className="inline-editor" onSubmit={(event) => { event.preventDefault(); if (!save.isPending) save.mutate(); }}>
          <strong>{t('工作区设置')}</strong>
          <label><span>{t('别名')}</span><input autoFocus data-autofocus disabled={save.isPending} value={editing.alias} onChange={(event) => setEditing({ ...editing, alias: event.target.value })} /></label>
          <label className="checkbox-label"><input type="checkbox" disabled={save.isPending} checked={editing.ignored} onChange={(event) => setEditing({ ...editing, ignored: event.target.checked })} />{t('从所有统计中忽略')}</label>
          <ActionFeedback pending={save.isPending} error={save.error} />
          <button type="submit" className="primary-button" disabled={save.isPending}><Save />{t('保存')}</button>
          <button type="button" className="quiet-button" disabled={save.isPending} onClick={() => setEditing(undefined)}>{t('取消')}</button>
        </form>
        </Dialog>
      )}
    </section>
  );
}
