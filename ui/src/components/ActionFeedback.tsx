import { AlertCircle, CheckCircle2, LoaderCircle } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { errorMessage } from '../lib/errors';

export function ActionFeedback({ error, pending, success }: {
  error?: unknown;
  pending?: boolean;
  success?: string | false;
}) {
  const { t } = useTranslation();
  if (pending) return <div className="action-feedback" role="status"><LoaderCircle className="spin" /><span>{t('正在处理，请稍候…')}</span></div>;
  if (error) return <div className="action-feedback error-feedback" role="alert"><AlertCircle /><span>{t('操作未完成')} · {errorMessage(error)} {t('请重试。')}</span></div>;
  if (success) return <div className="action-feedback success-feedback" role="status"><CheckCircle2 /><span>{t(success)}</span></div>;
  return null;
}
