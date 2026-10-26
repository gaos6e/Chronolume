export const agents = [
  { id: 'codex', label: 'Codex', detail: '本地会话与活动日志' },
  { id: 'claude', label: 'Claude Code', detail: '会话、缓存与子 agent' },
  { id: 'opencode', label: 'OpenCode', detail: '多提供方会话与工具活动' },
  { id: 'dsh', label: 'DeepSeek Harness', detail: '原生用量、缓存与工作节奏' },
] as const;

export type AgentId = typeof agents[number]['id'];
export function agentLabel(id?: string): string {
  return agents.find((agent) => agent.id === id)?.label ?? id ?? 'Codex';
}
