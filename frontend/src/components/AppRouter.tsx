import { useState, useEffect } from 'react';
import { OverviewPage } from './pages/OverviewPage';
import { CollectionsPage } from './pages/CollectionsPage';
import { CollectionEditorPage } from './pages/CollectionEditorPage';
import { RecordsBrowserPage } from './pages/RecordsBrowserPage';
import { SettingsPage } from './pages/SettingsPage';
import { AuthProvidersPage } from './pages/AuthProvidersPage';
import { LogsPage } from './pages/LogsPage';
import { BackupsPage } from './pages/BackupsPage';
import { ApiDocsPage } from './pages/ApiDocsPage';

function usePathname() {
  const [pathname, setPathname] = useState(() =>
    typeof window !== 'undefined' ? window.location.pathname : '/_/',
  );

  useEffect(() => {
    const onPopState = () => setPathname(window.location.pathname);
    window.addEventListener('popstate', onPopState);
    return () => window.removeEventListener('popstate', onPopState);
  }, []);

  return pathname;
}

function matchRoute(pathname: string) {
  const p = pathname.replace(/\/$/, '') || '/_';

  if (p === '/_') return { page: 'overview' };
  if (p === '/_/collections') return { page: 'collections' };
  if (p === '/_/collections/new') return { page: 'collection-new' };

  const editMatch = p.match(/^\/_\/collections\/([^/]+)\/edit$/);
  if (editMatch) return { page: 'collection-edit', id: editMatch[1] };

  const recordsMatch = p.match(/^\/_\/collections\/([^/]+)$/);
  if (recordsMatch) return { page: 'records', id: recordsMatch[1] };

  if (p === '/_/settings') return { page: 'settings' };
  if (p === '/_/settings/auth-providers') return { page: 'auth-providers' };
  if (p === '/_/logs') return { page: 'logs' };
  if (p === '/_/backups') return { page: 'backups' };
  if (p === '/_/docs') return { page: 'docs' };

  return { page: 'overview' };
}

export function AppRouter() {
  const pathname = usePathname();
  const route = matchRoute(pathname);

  switch (route.page) {
    case 'collections':
      return <CollectionsPage />;
    case 'collection-new':
      return <CollectionEditorPage mode="create" />;
    case 'collection-edit':
      return <CollectionEditorPage mode="edit" collectionId={route.id} />;
    case 'records':
      return <RecordsBrowserPage collectionId={route.id!} />;
    case 'settings':
      return <SettingsPage />;
    case 'auth-providers':
      return <AuthProvidersPage />;
    case 'logs':
      return <LogsPage />;
    case 'backups':
      return <BackupsPage />;
    case 'docs':
      return <ApiDocsPage />;
    default:
      return <OverviewPage />;
  }
}
