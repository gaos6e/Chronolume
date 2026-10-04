import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { App } from './App';
import { ErrorBoundary } from './components/ErrorBoundary';
import { initializeTheme } from './lib/theme';
import { readFontScale, readLocalPreference, storageKeys } from './lib/storage';
import './i18n';
import './styles.css';

initializeTheme();
document.documentElement.style.fontSize = `${readFontScale() * 100}%`;
document.documentElement.lang = readLocalPreference(storageKeys.language) === 'en' ? 'en' : 'zh-CN';

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 15_000,
      gcTime: 5 * 60_000,
      refetchOnWindowFocus: false,
    },
  },
});

const rootElement = document.getElementById('root');
if (!rootElement) {
  throw new Error('Missing application root element');
}

createRoot(rootElement).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <ErrorBoundary><App /></ErrorBoundary>
    </QueryClientProvider>
  </StrictMode>,
);
