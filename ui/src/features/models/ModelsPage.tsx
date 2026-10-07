import { useState } from 'react';
import { keepPreviousData, useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { CircleDollarSign, CloudDownload, Pencil, RotateCcw, Save, Trash2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import {
  applyPriceUpdate,
  deleteModelPrice,
  getModels,
  isTauriRuntime,
  listModelPrices,
  previewPriceUpdate,
  restoreBuiltinPrice,
  saveModelPrice,
} from '../../api';
import { PageControls } from '../../components/PageControls';
import { QueryStatus } from '../../components/QueryStatus';
import { ActionFeedback } from '../../components/ActionFeedback';
import { Dialog } from '../../components/Dialog';
import { useListState } from '../../lib/useListState';
import { invalidateUsageData } from '../../lib/queries';
import { formatCost, formatTokens, formatTps } from '../../lib/format';
import type { ModelPriceInput, UsageFilters } from '../../types';

const EMPTY_PRICE: ModelPriceInput = {
  provider: 'openai', pricingId: '', displayName: '', inputPerMillionUsd: '',
  outputPerMillionUsd: '', cacheReadPerMillionUsd: '', cacheWritePerMillionUsd: undefined,
};

export function ModelsPage({ filters }: { filters: UsageFilters }) {
  const { t } = useTranslation();
  const tpsDescription = `${t('模型平均 TPS = 总输出 Token ÷ 总活跃秒数，按时长加权。')} ${t('TPS = 输出 Token（含推理）÷ 活跃秒数；活跃时长包含工具调用等等待。')} ${t('已扣除可识别的用户答复等待。')}`;
  const queryClient = useQueryClient();
  const [tab, setTab] = useState<'usage' | 'prices'>('usage');
  const list = useListState(filters, 'name');
  const [editing, setEditing] = useState<ModelPriceInput>();
  const [updatePreview, setUpdatePreview] = useState<Awaited<ReturnType<typeof previewPriceUpdate>>>();
  const request = { filters, search: list.querySearch, sort: list.sort, descending: list.descending, page: list.page, pageSize: 30 };
  const models = useQuery({ queryKey: ['models', request], queryFn: () => getModels(request), placeholderData: keepPreviousData });
  const prices = useQuery({ queryKey: ['model-prices'], queryFn: () => listModelPrices(true), enabled: tab === 'prices' });
  const finishMutation = () => {
    setEditing(undefined);
    void queryClient.invalidateQueries({ queryKey: ['model-prices'] });
    invalidateUsageData(queryClient);
  };
  const save = useMutation({ mutationFn: (input: ModelPriceInput) => saveModelPrice(input), onSuccess: finishMutation });
  const remove = useMutation({ mutationFn: ({ provider, pricingId }: { provider: string; pricingId: string }) => deleteModelPrice(provider, pricingId), onSuccess: finishMutation });
  const restore = useMutation({ mutationFn: ({ provider, pricingId }: { provider: string; pricingId: string }) => restoreBuiltinPrice(provider, pricingId), onSuccess: finishMutation });
  const checkUpdate = useMutation({ mutationFn: previewPriceUpdate, onSuccess: setUpdatePreview });
  const applyUpdate = useMutation({
    mutationFn: (previewId: string) => applyPriceUpdate(previewId),
    onSuccess: () => { setUpdatePreview(undefined); finishMutation(); },
  });
  const busy = save.isPending || remove.isPending || restore.isPending || applyUpdate.isPending;
  const resetFeedback = () => { save.reset(); remove.reset(); restore.reset(); applyUpdate.reset(); };
  const openEditor = (value: ModelPriceInput) => { resetFeedback(); setEditing(value); };

  return (
    <section className="feature-page">
      <div className="tabs" role="tablist" aria-label={t('模型与成本')} onKeyDown={(event) => {
        if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
        event.preventDefault();
        const next = event.key === 'Home' ? 'usage' : event.key === 'End' ? 'prices' : tab === 'usage' ? 'prices' : 'usage';
        setTab(next);
        event.currentTarget.querySelector<HTMLButtonElement>(`#${next}-tab`)?.focus();
      }}>
        <button id="usage-tab" aria-controls="models-panel" tabIndex={tab === 'usage' ? 0 : -1} type="button" role="tab" aria-selected={tab === 'usage'} className={tab === 'usage' ? 'active' : ''} onClick={() => setTab('usage')}>{t('模型用量')}</button>
        <button id="prices-tab" aria-controls="models-panel" tabIndex={tab === 'prices' ? 0 : -1} type="button" role="tab" aria-selected={tab === 'prices'} className={tab === 'prices' ? 'active' : ''} onClick={() => setTab('prices')}>{t('价格表')}</button>
      </div>
      <div id="models-panel" role="tabpanel" aria-labelledby={`${tab}-tab`} className="tab-panel">
      {tab === 'usage' ? <>
        <PageControls
          {...list} onPage={list.setPage} loading={models.isFetching} pageSize={30}
          total={models.data?.total ?? 0}
          sortOptions={[{ value: 'tokens', label: 'Token' }, { value: 'tps', label: t('平均 TPS') }, { value: 'cost', label: t('成本') }, { value: 'sessions', label: t('会话') }, { value: 'recent', label: t('最近使用') }, { value: 'name', label: t('名称') }]}
        />
        <QueryStatus loading={models.isLoading && !models.data} refreshing={models.isFetching && Boolean(models.data)} error={models.error} onRetry={() => void models.refetch()} />
        {models.data && (
        <div className="data-table-wrap" role="region" aria-label={t('模型用量')} tabIndex={0} aria-busy={models.isFetching}><table className="data-table">
          <thead><tr><th>{t('模型')}</th><th>{t('会话')}</th><th>{t('输入')}</th><th>{t('缓存')}</th><th>{t('输出')}</th><th>{t('推理')}</th><th title={tpsDescription}>{t('平均 TPS')}</th><th>{t('命中率')}</th><th>{t('成本')}</th><th>{t('平均每百万 Token 成本')}</th><th>{t('最近使用')}</th></tr></thead>
          <tbody>{models.data?.items.map((model) => <tr key={model.model}>
            <td><strong>{model.model}</strong><small>{model.pricingModelId ? `${t('计价')} ${model.pricingModelId}` : t('未匹配价格')}</small></td>
            <td>{model.sessionCount.toLocaleString()}</td><td>{formatTokens(model.freshInputTokens)}</td>
            <td>{formatTokens(model.cachedInputTokens)}</td><td>{formatTokens(model.outputTokens)}</td>
            <td>{formatTokens(model.reasoningTokens)}</td>
            <td className="tps-value" title={tpsDescription}>{formatTps(model.averageTokensPerSecond)}</td>
            <td>{model.cacheHitRate == null ? 'N/A' : `${(model.cacheHitRate * 100).toFixed(1)}%`}</td>
            <td>{formatCost(model.estimatedCostMicrousd)}{model.unpricedEventCount > 0 && <small className="warning-copy">{t('部分未定价')}</small>}</td>
            <td>{formatCost(model.averageCostMicrousdPerMillionTokens)}</td>
            <td>{model.lastUsedAtMs ? new Date(model.lastUsedAtMs).toLocaleString() : '—'}</td>
          </tr>)}</tbody>
        </table>{!models.isLoading && models.data?.items.length === 0 && <div className="table-empty">{t('没有匹配的模型。')}</div>}</div>
        )}
      </> : <>
        <div className="pricing-toolbar">
          <p>{t('价格单位为 USD / 1M Token。修改后只重算本地汇总，不重新读取 JSONL。')}</p>
          <div className="toolbar-actions">
            <button type="button" className="quiet-button" disabled={busy || checkUpdate.isPending || !isTauriRuntime()} onClick={() => { resetFeedback(); checkUpdate.mutate(); }}><CloudDownload />{checkUpdate.isPending ? t('正在检查') : t('检查官方更新')}</button>
            <button type="button" className="primary-button" disabled={busy || !isTauriRuntime()} onClick={() => openEditor(EMPTY_PRICE)}><CircleDollarSign />{t('添加价格')}</button>
          </div>
        </div>
        <ActionFeedback error={checkUpdate.error ?? remove.error ?? restore.error} pending={checkUpdate.isPending || remove.isPending || restore.isPending} />
        <ActionFeedback success={(save.isSuccess || remove.isSuccess || restore.isSuccess || applyUpdate.isSuccess) && '价格已更新，本地统计已重算。'} />
        <QueryStatus loading={prices.isLoading && !prices.data} refreshing={prices.isFetching && Boolean(prices.data)} error={prices.error} onRetry={() => void prices.refetch()} />
        {prices.data && (
        <div className="data-table-wrap" role="region" aria-label={t('价格表')} tabIndex={0}><table className="data-table">
          <thead><tr><th>{t('计价 ID')}</th><th>{t('输入')}</th><th>{t('缓存读')}</th><th>{t('缓存写')}</th><th>{t('输出')}</th><th>{t('来源')}</th><th><span className="sr-only">{t('操作')}</span></th></tr></thead>
          <tbody>{prices.data?.map((price) => <tr key={`${price.provider}-${price.pricingId}`} className={price.isDeleted ? 'deleted-row' : ''}>
            <td><strong>{price.displayName}</strong><small>{price.provider} / {price.pricingId}</small></td>
            <td>${price.inputPerMillionUsd}</td><td>${price.cacheReadPerMillionUsd}</td>
            <td>{price.cacheWritePerMillionUsd ? `$${price.cacheWritePerMillionUsd}` : 'N/A'}</td><td>${price.outputPerMillionUsd}</td>
            <td>{price.isBuiltin ? t('内置官方快照') : t('用户价格')}{price.isDeleted && <small className="warning-copy">{t('已删除')}</small>}{price.isOverridden && <small>{t('已覆盖')}</small>}{price.pricingId === 'gpt-5.5' && <small>{t('标准价适用于不超过 272K 输入')}</small>}</td>
            <td><div className="row-actions">
              {!price.isDeleted && <button type="button" className="icon-button" disabled={busy} aria-label={t('编辑价格')} onClick={() => openEditor({
                provider: price.provider, pricingId: price.pricingId, displayName: price.displayName,
                inputPerMillionUsd: price.inputPerMillionUsd, outputPerMillionUsd: price.outputPerMillionUsd,
                cacheReadPerMillionUsd: price.cacheReadPerMillionUsd, cacheWritePerMillionUsd: price.cacheWritePerMillionUsd,
              })}><Pencil /></button>}
              {!price.isDeleted && <button type="button" className="icon-button danger" disabled={busy} aria-label={t('删除价格')} onClick={() => { resetFeedback(); remove.mutate(price); }}><Trash2 /></button>}
              {price.isBuiltin && (price.isDeleted || price.isOverridden) && <button type="button" className="icon-button" disabled={busy} aria-label={t('恢复内置价格')} onClick={() => { resetFeedback(); restore.mutate(price); }}><RotateCcw /></button>}
            </div></td>
          </tr>)}</tbody>
        </table>{!prices.isLoading && prices.data?.length === 0 && <div className="table-empty">{t('价格表为空。')}</div>}</div>
        )}
      </>}
      </div>
      {editing && <PriceEditor value={editing} busy={save.isPending} error={save.error} onChange={setEditing} onCancel={() => setEditing(undefined)} onSave={() => save.mutate(editing)} />}
      {updatePreview && <Dialog className="price-preview" label={t('价格更新预览')} busy={applyUpdate.isPending} onClose={() => setUpdatePreview(undefined)}>
        <div><strong>{t('OpenAI 官方价格差异')}</strong><small>{new Date(updatePreview.fetchedAtMs).toLocaleString()} · {updatePreview.unchangedCount} {t('项未变化')}</small><a href={updatePreview.sourceUrl} target="_blank" rel="noreferrer">{t('查看固定可信来源')}</a></div>
        <div className="price-change-list">{updatePreview.changes.length === 0 ? <p>{t('本地内置价格已是最新。')}</p> : updatePreview.changes.map((change) => <div key={change.pricingId}><span>{change.kind === 'added' ? t('新增') : t('更新')} · {change.pricingId}</span><strong>{change.before ? `$${change.before.inputPerMillionUsd} → ` : ''}${change.after.inputPerMillionUsd} {t('输入')} / ${change.after.outputPerMillionUsd} {t('输出')}</strong></div>)}</div>
        <ActionFeedback pending={applyUpdate.isPending} error={applyUpdate.error} />
        <button type="button" className="primary-button" disabled={applyUpdate.isPending || updatePreview.changes.length === 0} onClick={() => applyUpdate.mutate(updatePreview.previewId)}>{t('确认应用并重算')}</button>
        <button type="button" className="quiet-button" disabled={applyUpdate.isPending} onClick={() => setUpdatePreview(undefined)}>{t('取消')}</button>
      </Dialog>}
    </section>
  );
}

function PriceEditor({ value, busy, error, onChange, onCancel, onSave }: {
  value: ModelPriceInput; busy: boolean; onChange: (value: ModelPriceInput) => void;
  onCancel: () => void; onSave: () => void;
  error: unknown;
}) {
  const { t } = useTranslation();
  const field = (key: keyof ModelPriceInput, label: string, required = true) => {
    const numeric = key.endsWith('PerMillionUsd');
    return <label><span>{t(label)}</span><input autoFocus={key === 'pricingId'} data-autofocus={key === 'pricingId' ? true : undefined} disabled={busy} type={numeric ? 'number' : 'text'} min={numeric ? '0' : undefined} step={numeric ? 'any' : undefined} required={required} value={value[key] ?? ''} onChange={(event) => onChange({ ...value, [key]: event.target.value || undefined })} /></label>;
  };
  return <Dialog className="editor-dialog price-editor-dialog" label={t('模型价格')} busy={busy} onClose={onCancel}><form className="inline-editor price-editor" onSubmit={(event) => { event.preventDefault(); if (!busy) onSave(); }}>
    <strong>{t('模型价格')}</strong>{field('provider', '提供方')}{field('pricingId', '计价 ID')}{field('displayName', '显示名称')}
    {field('inputPerMillionUsd', '输入')}{field('cacheReadPerMillionUsd', '缓存读取')}{field('cacheWritePerMillionUsd', '缓存写入（可选）', false)}{field('outputPerMillionUsd', '输出')}
    <p className="form-hint">{t('价格单位为 USD / 1M Token。')}</p>
    <ActionFeedback pending={busy} error={error} />
    <button type="submit" className="primary-button" disabled={busy}><Save />{t('保存并重算')}</button><button type="button" className="quiet-button" disabled={busy} onClick={onCancel}>{t('取消')}</button>
  </form></Dialog>;
}
