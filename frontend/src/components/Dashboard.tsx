import { DashboardLayout } from './DashboardLayout';

/**
 * Top-level React island for the dashboard page.
 * Uses the full dashboard layout with sidebar, header, and auth guard.
 *
 * @deprecated Use page-specific components (CollectionsPage, SettingsPage, etc.) instead.
 */
export function Dashboard() {
  return (
    <DashboardLayout currentPath="/_/" pageTitle="Dashboard">
      <p className="text-secondary">Welcome to the Zerobase admin dashboard.</p>
    </DashboardLayout>
  );
}
