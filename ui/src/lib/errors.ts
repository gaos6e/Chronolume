import i18n from '../i18n';

// Tauri serializes the backend's sanitized AppError as an object, not an Error.
export function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === 'string' && error.trim()) return i18n.t(error);
  if (error && typeof error === 'object' && 'message' in error && typeof error.message === 'string') {
    return i18n.t(error.message);
  }
  return i18n.t('发生未知错误。');
}
