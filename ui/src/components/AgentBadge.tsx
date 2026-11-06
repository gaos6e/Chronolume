import { Asterisk, Command, SquareTerminal, Waves, Bot } from 'lucide-react';
import { agentLabel } from '../lib/agents';

export function AgentMark({ agent }: { agent?: string }) {
  const Icon = agent === 'claude' ? Asterisk : agent === 'opencode' ? Command : agent === 'dsh' ? Waves : agent === 'workbuddy' ? Bot : SquareTerminal;
  return <Icon className={`agent-mark ${agent ?? 'codex'}`} aria-hidden="true" />;
}

export function AgentBadge({ agent }: { agent?: string }) {
  return <span className={`agent-badge ${agent ?? 'codex'}`}><AgentMark agent={agent} />{agentLabel(agent)}</span>;
}
