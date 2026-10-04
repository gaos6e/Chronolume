import { useTranslation } from 'react-i18next';
import { errorMessage } from '../lib/errors';

export function QueryStatus({
  loading,
  error,
  onRetry,
  refreshing = false,
}: {
  loading: boolean;
  error: unknown;
  onRetry: () => void;
  refreshing?: boolean;
}) {
  const { t } = useTranslation();
  if (loading) {
    return <div className="inline-loading" role="status">{t('正在加载页面数据…')}</div>;
  }
  if (!error) return refreshing ? <span className="sr-only" role="status">{t('正在刷新查询')}</span> : null;
  return (
    <section className="state-card error-state" role="alert">
      <strong>{t('无法读取页面数据')}</strong>
      <p>{errorMessage(error)}</p>
      <button type="button" onClick={onRetry}>{t('重试查询')}</button>
    </section>
  );
}
