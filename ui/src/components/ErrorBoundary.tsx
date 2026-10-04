import { Component, type ReactNode } from 'react';
import i18n from '../i18n';

export class ErrorBoundary extends Component<{ children: ReactNode }, { failed: boolean }> {
  state = { failed: false };
  static getDerivedStateFromError() { return { failed: true }; }
  render() {
    if (!this.state.failed) return this.props.children;
    return <section className="state-card error-state" role="alert">
      <strong>{i18n.t('界面加载失败')}</strong>
      <p>{i18n.t('请重新加载应用后重试；本地原始数据不会受到影响。')}</p>
      <button type="button" onClick={() => window.location.reload()}>{i18n.t('重新加载')}</button>
    </section>;
  }
}
