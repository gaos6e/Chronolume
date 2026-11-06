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
import type { SourceSettings } from '../../types';

export function SourceConnections({ syncing, onSaved }: { syncing: boolean; onSaved: () => void }) {
  const { t } = useTranslation();
  const client = useQueryClient();
  const sources = useQuery({ queryKey: ['source-settings'], queryFn: getSourceSettings });
  const [editing, setEditing] = useState<AgentId>();
  const [path, setPath] = useState('');
  const [aiPath, setAiPath] = useState('');
  const [browseError, setBrowseError] = useState<unknown>();
  const save = useMutation({ mutationFn: (settings: SourceSettings) => saveSourceSettings(settings),
    onSuccess: (saved) => { client.setQueryData(['source-settings'], saved); setEditing(undefined); onSaved(); },
  });
  const browse = async (ai = false) => {
    setBrowseError(undefined);
    try {
      const selected = await open({ directory: true, multiple: false, title: t('选择数据目录') });
      if (typeof selected === 'string') (ai ? setAiPath : setPath)(selected);
    } catch (error) { setBrowseError(error); }
  };
  const submit = (agent: AgentId) => {
    const settings = Object.fromEntries((sources.data ?? []).map((source) => [source.agent, source.customPath ?? null])) as SourceSettings;
    const workbuddy = sources.data?.find((source) => source.agent === 'workbuddy');
    if (workbuddy) settings.workbuddyAi = workbuddy.roots?.find((root) => root.settingKey === 'workbuddyAi')?.customPath ?? null;
    save.mutate({ ...settings, [agent]: path.trim() || null, ...(agent === 'workbuddy' ? { workbuddyAi: aiPath.trim() || null } : {}) });
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
        {source?.roots?.length ? <div className="connection-roots">{source.roots.map((root) => <div className="connection-path" key={root.settingKey}><span><i className={root.available ? 'root-detected' : ''} />{root.label}</span><code title={root.path}>{root.path}</code></div>)}</div> : <div className="connection-path"><span>{source?.customPath ? t('自定义目录') : t('自动发现')}</span><code title={source?.path}>{source?.path ?? '—'}</code></div>}
        <button type="button" className="connection-configure" disabled={!isTauriRuntime() || syncing || save.isPending || !source} aria-expanded={editing === agent.id} onClick={() => { save.reset(); setBrowseError(undefined); setEditing(editing === agent.id ? undefined : agent.id); setPath(source?.customPath ?? ''); setAiPath(source?.roots?.find((root) => root.settingKey === 'workbuddyAi')?.customPath ?? ''); }}><Settings2 />{t('配置目录')}<ChevronRight /></button>
      </article>;
    })}</div>
    {editing && <form className="source-path-editor" onSubmit={(event) => { event.preventDefault(); submit(editing); }}>
      <label htmlFor="source-path"><span>{agents.find((agent) => agent.id === editing)?.label} · {t('数据目录')}</span><input id="source-path" autoFocus value={path} onChange={(event) => setPath(event.target.value)} disabled={save.isPending || syncing} placeholder={sources.data?.find((source) => source.agent === editing)?.path} /></label>
      {editing === 'workbuddy' && <label htmlFor="source-path-ai"><span>WorkBuddy AI · {t('数据目录')}</span><div className="source-ai-input"><input id="source-path-ai" value={aiPath} onChange={(event) => setAiPath(event.target.value)} disabled={save.isPending || syncing} placeholder={sources.data?.find((source) => source.agent === 'workbuddy')?.roots?.find((root) => root.settingKey === 'workbuddyAi')?.path} /><button type="button" className="quiet-button" disabled={save.isPending || syncing} onClick={() => void browse(true)}><FolderOpen />{t('浏览')}</button></div></label>}
      <div className="source-path-actions"><button type="button" className="quiet-button" disabled={save.isPending || syncing} onClick={() => void browse()}><FolderOpen />{t('浏览')}</button><button type="button" className="quiet-button" disabled={save.isPending || syncing} onClick={() => { setPath(''); if (editing === 'workbuddy') setAiPath(''); }}><RotateCcw />{t('恢复自动发现')}</button><button type="submit" className="primary-button" disabled={save.isPending || syncing}>{t('保存并同步')}</button></div>
      <p className="form-hint">{t('留空使用环境变量或默认目录。OpenCode 也可填写数据库文件的绝对路径。')}</p>
      {editing === 'workbuddy' && <p className="form-hint">{t('两个版本合并展示；只统计原生用量，缺少原生计时显示为 —。')}</p>}
      <ActionFeedback pending={save.isPending} error={save.error ?? browseError} />
    </form>}
    <p className="connections-footnote"><LockKeyhole />{t('不读取账号凭据，不安装 hook，不修改 agent 的原始数据。')}</p>
  </section>;
}
