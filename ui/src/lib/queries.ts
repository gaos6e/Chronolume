import type { QueryClient } from '@tanstack/react-query';

// Workspace exclusions and price recalculation affect every usage view and export context.
export function invalidateUsageData(client: QueryClient): void {
  ['dashboard', 'workspaces', 'workspace-catalog', 'sessions', 'session-detail',
    'usage-events', 'models', 'heatmap', 'tools', 'diagnostics', 'bootstrap'].forEach((root) => {
    void client.invalidateQueries({ queryKey: [root] });
  });
}
