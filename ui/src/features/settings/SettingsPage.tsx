import { useEffect, useState } from 'react';
import { Languages, MonitorCog, Moon, Sun, Type } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { getAppPreferences, isTauriRuntime, saveAppPreferences } from '../../api';
import { QueryStatus } from '../../components/QueryStatus';
import { ActionFeedback } from '../../components/ActionFeedback';
import { applyThemePreference, readThemePreference, type ThemePreference } from '../../lib/theme';
import { readFontScale, readLocalPreference, storageKeys, writeLocalPreference } from '../../lib/storage';

export function SettingsPage() {
  const { t, i18n } = useTranslation();
  const queryClient = useQueryClient();
  const [theme, setTheme] = useState<ThemePreference>(readThemePreference);
  const [language, setLanguage] = useState(() => readLocalPreference(storageKeys.language) ?? 'zh-CN');
  const [fontScale, setFontScale] = useState(readFontScale);
  const [idleGapMinutes, setIdleGapMinutes] = useState('30');
  const idleValue = Number(idleGapMinutes);
  const validIdle = idleGapMinutes !== '' && Number.isInteger(idleValue) && idleValue >= 1 && idleValue <= 240;
  const preferences = useQuery({ queryKey: ['app-preferences'], queryFn: getAppPreferences });
  const savePreferences = useMutation({
    mutationFn: () => saveAppPreferences({
      idleGapMinutes: idleValue,
      visibleWorkspaceIds: preferences.data?.visibleWorkspaceIds ?? [],
    }),
    onSuccess: (saved) => queryClient.setQueryData(['app-preferences'], saved),
  });

  useEffect(() => {
    applyThemePreference(theme);
  }, [theme]);
  useEffect(() => {
    writeLocalPreference(storageKeys.language, language);
    document.documentElement.lang = language;
    void i18n.changeLanguage(language);
  }, [i18n, language]);
  useEffect(() => {
    const safeScale = Math.max(.9, Math.min(1.35, fontScale));
    writeLocalPreference(storageKeys.fontScale, String(safeScale));
    document.documentElement.style.fontSize = `${safeScale * 100}%`;
  }, [fontScale]);
  useEffect(() => {
    if (preferences.data) setIdleGapMinutes(String(preferences.data.idleGapMinutes));
  }, [preferences.data]);

  return <section className="feature-page settings-page">
    <QueryStatus loading={preferences.isLoading && !preferences.data} error={preferences.error} onRetry={() => void preferences.refetch()} />
    <section className="feature-card setting-row"><div><MonitorCog /><span><strong>{t('主题')}</strong><small>{t('暗色、浅色或跟随系统')}</small></span></div><div className="choice-buttons">
      <button type="button" aria-pressed={theme === 'system'} className={theme === 'system' ? 'active' : ''} onClick={() => setTheme('system')}><MonitorCog />{t('系统')}</button>
      <button type="button" aria-pressed={theme === 'dark'} className={theme === 'dark' ? 'active' : ''} onClick={() => setTheme('dark')}><Moon />{t('暗色')}</button>
      <button type="button" aria-pressed={theme === 'light'} className={theme === 'light' ? 'active' : ''} onClick={() => setTheme('light')}><Sun />{t('浅色')}</button>
    </div></section>
    <section className="feature-card setting-row"><div><Languages /><span><strong>{t('语言')}</strong><small>{t('界面语言偏好')}</small></span></div><select aria-label={t('语言')} value={language} onChange={(event) => setLanguage(event.target.value)}><option value="zh-CN">简体中文</option><option value="en">English</option></select></section>
    <section className="feature-card setting-row"><div><Type /><span><strong>{t('字体缩放')}</strong><small>{Math.round(fontScale * 100)}%</small></span></div><input aria-label={t('字体缩放')} type="range" min="0.9" max="1.35" step="0.05" value={fontScale} onChange={(event) => setFontScale(Number(event.target.value))} /></section>
    <section className="feature-card setting-row"><div><MonitorCog /><span><strong>{t('活跃时间空闲间隔')}</strong><small id="idle-gap-hint">{t('缺少任务生命周期事件时用于估算；1–240 分钟')}</small></span></div><form className="setting-action" onSubmit={(event) => { event.preventDefault(); if (validIdle && preferences.data && !savePreferences.isPending) savePreferences.mutate(); }}><input aria-label={t('活跃时间空闲间隔')} aria-describedby={!validIdle ? 'idle-gap-hint idle-gap-error' : 'idle-gap-hint'} aria-invalid={!validIdle} type="number" required min="1" max="240" step="1" disabled={!isTauriRuntime() || !preferences.data || savePreferences.isPending} value={idleGapMinutes} onChange={(event) => { savePreferences.reset(); setIdleGapMinutes(event.target.value); }} /><span>{t('分钟')}</span><button type="submit" className="primary-button" disabled={!isTauriRuntime() || !preferences.data || !validIdle || savePreferences.isPending || idleValue === preferences.data.idleGapMinutes}>{t('保存')}</button></form>
      {!validIdle && <p className="form-error" id="idle-gap-error" role="alert">{t('请输入 1–240 之间的整数。')}</p>}
      <ActionFeedback pending={savePreferences.isPending} error={savePreferences.error} success={savePreferences.isSuccess && '偏好已保存，将用于后续同步的活跃时间估算。'} />
    </section>
    <section className="feature-card about-card"><h2>Chronolume 2.1.9</h2><p>{t('Chronolume 照亮本机 Codex 活动中的 Token、成本与时间脉络。应用不会读取 auth.json、在线配额或对话正文；价格更新是唯一可选网络能力，且只在用户明确触发后执行。')}</p></section>
  </section>;
}
