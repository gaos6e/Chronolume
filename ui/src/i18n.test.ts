import { afterEach, describe, expect, it } from 'vitest';
import i18n from './i18n';

describe('i18n', () => {
  afterEach(async () => { await i18n.changeLanguage('zh-CN'); });

  it('switches core navigation and privacy copy between Chinese and English', async () => {
    await i18n.changeLanguage('en');
    expect(i18n.t('本地用量总览')).toBe('Local usage overview');
    expect(i18n.t('匿名路径')).toBe('Anonymous paths');
    expect(i18n.t('平均 TPS')).toBe('Average TPS');
    expect(i18n.t('平均首字延迟')).toBe('Average TTFT');
    expect(i18n.t('有效轮次：{{count}}', { count: 2 })).toBe('Valid turns: 2');
    expect(i18n.t('TPS = 输出 Token（含推理）÷ 活跃秒数；活跃时长包含工具调用等等待。')).toContain('including reasoning');
    expect(i18n.t('模型平均 TPS = 有计时记录的总输出 Token ÷ 总活跃秒数，按时长加权。')).toContain('timed records');
    expect(i18n.t('进行中轮次按最新日志估算活跃时长，结束后更新为正式计时。')).toContain('latest log event');
    expect(i18n.t('已扣除可识别的用户答复等待。')).toBe('Identifiable pauses for user answers are excluded.');
    await i18n.changeLanguage('zh-CN');
    expect(i18n.t('本地用量总览')).toBe('本地用量总览');
    expect(i18n.t('平均 TPS')).toBe('平均 TPS');
  });
});
