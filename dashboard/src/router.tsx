import { createRouter, createRoute, createRootRoute } from '@tanstack/react-router';
import { AppShell } from './components/layout/AppShell';
import { DashboardPage } from './components/dashboard/DashboardPage';
import { TasksPage } from './components/tasks/TasksPage';
import { McpPage } from './components/mcp/McpPage';
import { SchedulerPage } from './components/scheduler/SchedulerPage';
import { ConfigPage } from './components/config/ConfigPage';
import { SessionViewer } from './components/viewer/SessionViewer';
import { LoginPage } from './components/LoginPage';

const rootRoute = createRootRoute({ component: AppShell });

const loginRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/login',
    component: LoginPage,
});

const indexRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/',
    component: DashboardPage,
});

const tasksRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/tasks',
    component: TasksPage,
});

export const sessionRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/tasks/$id',
    component: SessionViewer,
});

// Keep backwards compat for old /session/$id URL
export const legacySessionRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/session/$id',
    component: SessionViewer,
});

const mcpRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/mcp',
    component: McpPage,
});

const schedulerRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/scheduler',
    component: SchedulerPage,
});

const configRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/config',
    component: ConfigPage,
});

const routeTree = rootRoute.addChildren([
    loginRoute,
    indexRoute,
    tasksRoute,
    sessionRoute,
    legacySessionRoute,
    mcpRoute,
    schedulerRoute,
    configRoute,
]);

export const router = createRouter({ routeTree });

declare module '@tanstack/react-router' {
    interface Register {
        router: typeof router;
    }
}
