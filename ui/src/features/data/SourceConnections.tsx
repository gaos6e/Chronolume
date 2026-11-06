import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { open } from '@tauri-apps/plugin-dialog';
import { Check, ChevronRight, FolderOpen, LockKeyhole, RotateCcw, Settings2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { getSourceSettings, isTauriRuntime, saveSourceSettings } from '../../api';
import { AgentMark } from '../../components/AgentBadge';
import { ActionFeedback } from '../../components/ActionFeedback';
import { QueryStatus } from '../../components/QueryStatus';
import { agents, type AgentId } from '../../lib/agents';
import type { AgentSourceSetting, SourceSettings } from '../../types';

export function SourceConnections({ syncing, onSaved }: { syncing: boolean; onSaved: () => void }) {
  const { t } = useTranslation();
  const client = useQueryClient();
  const sources = useQuery({ queryKey: ['source-settings'], queryFn: getSourceSettings });
  const [editing, setEditing] = useState<AgentId>();
  const [draft, setDraft] = useState<SourceSettings>({});
  const [browseError, setBrowseError] = useState<unknown>();
  const save = useMutation({ mutationFn: (settings: SourceSettings) => saveSourceSettings(settings),
    onSuccess: (saved) => { client.setQueryData(['source-settings'], saved); setEditing(undefined); onSaved(); },
  });
  const setPath = (key: keyof SourceSettings, path: string) => setDraft((current) => ({ ...current, [key]: path }));
  const browse = async (key: keyof SourceSettings) => {
    setBrowseError(undefined);
    try {
      const selected = await open({ directory: true, multiple: false, title: t('选择数据目录') });
      if (typeof selected === 'string') setPath(key, selected);
    } catch (error) { setBrowseError(error); }
  };
  const directoryFields = (agent: AgentId, source?: AgentSourceSetting) => source?.roots?.length
    ? source.roots
    : [{ settingKey: agent, label: agents.find((item) => item.id === agent)?.label, path: source?.path, customPath: source?.customPath }];
  const edit = (agent: AgentId, source?: AgentSourceSetting) => {
    save.reset();
    setBrowseError(undefined);
    setEditing(editing === agent ? undefined : agent);
    setDraft(Object.fromEntries(directoryFields(agent, source).map((field) => [field.settingKey, field.customPath ?? ''])));
  };
  const fields = editing ? directoryFields(editing, sources.data?.find((source) => source.agent === editing)) : [];
  const submit = () => {
    const settings: SourceSettings = {};
    for (const agent of agents) {
      const source = sources.data?.find((entry) => entry.agent === agent.id);
      if (!source) continue;
      for (const field of directoryFields(agent.id, source)) settings[field.settingKey] = field.customPath ?? null;
    }
    for (const field of fields) settings[field.settingKey] = draft[field.settingKey]?.trim() || null;
    save.mutate(settings);
  };
  return <section className="feature-card connections-section">
    <div className="feature-card-heading"><div><span>YOUR CODING AGENTS</span><h2>{t('连接你的工作节奏')}</h2></div><span className="local-only-label"><LockKeyhole />{t('仅本机 · 只读')}</span></div>
    <p className="connections-description">{t('在同一视图中看见不同 agent 的投入。自动发现本地记录，也可指定数据目录。')}</p>
    <QueryStatus loading={sources.isLoading && !sources.data} error={sources.error} onRetry={() => void sources.refetch()} />
    <div className="agent-connections">{agents.map((agent) => {
      const source = sources.data?.find((source) => source.agent === agent.id);
      const active = Boolean(source?.available);
      return <article key={agent.id} className={`agent-connection ${agent.id}${editing === agent.id ? ' is-editing' : ''}`}>
        <div className="connection-top"><span className="connection-icon"><AgentMark agent={agent.id} /></span><span className={`connection-status${active ? ' detected' : ''}`}>{active ? <Check /> : <span className="status-dot" />}{active ? t('已检测') : t('未检测')}</span></div>
        <h3>{agent.label}</h3><p>{t(agent.detail)}</p>
        {source?.roots?.length ? <div className="connection-roots">{source.roots.map((root) => <div className="connection-path" key={root.settingKey}><span><i className={root.available ? 'root-detected' : ''} />{root.label}<small className="connection-root-status">{t(root.available ? '已检测' : root.path ? '暂无记录' : '未配置')}</small></span>{root.path && <code title={root.path}>{root.path}</code>}</div>)}</div> : <div className="connection-path"><span>{source?.customPath ? t('自定义目录') : t('自动发现')}</span><code title={source?.path}>{source?.path ?? '—'}</code></div>}
        <button type="button" className="connection-configure" disabled={!isTauriRuntime() || syncing || save.isPending || !source} aria-expanded={editing === agent.id} onClick={() => edit(agent.id, source)}><Settings2 />{t('配置目录')}<ChevronRight /></button>
      </article>;
    })}</div>
    {editing && <form className="source-path-editor" onSubmit={(event) => { event.preventDefault(); submit(); }}>
      {fields.map((field, index) => {
        const inputId = `source-path-${field.settingKey}`;
        return <div className="source-path-field" key={field.settingKey}>
          <label htmlFor={inputId}>{field.label} · {t(field.settingKey === 'workbuddyProxy' ? '日志目录' : '数据目录')}</label>
          <div className="source-path-input">
            <input id={inputId} autoFocus={index === 0} value={draft[field.settingKey] ?? ''} onChange={(event) => setPath(field.settingKey, event.target.value)} disabled={save.isPending || syncing} placeholder={field.path || t('选择日志目录')} />
            <button type="button" className="quiet-button" disabled={save.isPending || syncing} onClick={() => void browse(field.settingKey)}><FolderOpen />{t('浏览')}</button>
          </div>
        </div>;
      })}
      <div className="source-path-actions"><button type="button" className="quiet-button" disabled={save.isPending || syncing} onClick={() => setDraft({})}><RotateCcw />{t('恢复自动发现')}</button><button type="submit" className="primary-button" disabled={save.isPending || syncing}>{t('保存并同步')}</button></div>
      <p className="form-hint">{t('留空使用环境变量或默认目录。OpenCode 也可填写数据库文件的绝对路径。')}</p>
      {editing === 'workbuddy' && <p className="form-hint">{t('桌面版、CLI 和已配置的代理日志合并展示。代理只统计有时间及用量的成功请求，留空则不启用。')}</p>}
      <ActionFeedback pending={save.isPending} error={save.error ?? browseError} />
    </form>}
    <p className="connections-footnote"><LockKeyhole />{t('不读取账号凭据，不安装 hook，不修改 agent 的原始数据。')}</p>
  </section>;
}
