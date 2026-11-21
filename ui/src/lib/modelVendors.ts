import i18n from '../i18n';

const labels: Record<string, string> = {
  openai: 'OpenAI', anthropic: 'Anthropic', deepseek: 'DeepSeek', google: 'Google Gemini',
  zai: 'Z.AI · GLM', minimax: 'MiniMax', moonshotai: 'Kimi', alibaba: 'Qwen', xai: 'Grok',
  tencent: 'Tencent · Hunyuan', xiaomi: 'Xiaomi · MiMo', nvidia: 'NVIDIA',
};

export function modelVendorLabel(vendor: string): string {
  return vendor === 'unknown' ? i18n.t('未识别供应商') : labels[vendor] ?? vendor;
}
