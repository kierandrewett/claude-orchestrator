import { createRouter, createRoute, createRootRoute, Outlet } from '@tanstack/react-router';
import { useSSE } from './hooks/useSSE';
import { Header } from './components/layout/Header';
import { Sidebar } from './components/layout/Sidebar';
import { SessionViewer } from './components/viewer/SessionViewer';

// ── Root layout ───────────────────────────────────────────────────────────────

function RootLayout() {
    useSSE();

    return (
        <div className="flex flex-col bg-zinc-950 text-zinc-100 overflow-hidden" style={{ height: '100dvh' }}>
            <Header />
            <div className="flex flex-1 overflow-hidden min-h-0">
                {/* Sidebar — desktop only */}
                <aside className="hidden lg:flex w-72 flex-col shrink-0 bg-zinc-900 border-r border-zinc-800">
                    <Sidebar onNavigate={() => {}} />
                </aside>

                <main className="flex-1 overflow-hidden flex flex-col min-h-0">
                    <Outlet />
                </main>
            </div>
        </div>
    );
}

// ── Home page ─────────────────────────────────────────────────────────────────

function HomePage() {
    return (
        <>
            {/* Mobile: full sessions list */}
            <div className="lg:hidden flex-1 overflow-hidden flex flex-col min-h-0">
                <Sidebar onNavigate={() => {}} />
            </div>

            {/* Desktop: placeholder */}
            <div className="hidden lg:flex flex-col items-center justify-center h-full text-zinc-500 gap-4">
                <div className="w-12 h-12 rounded-full bg-zinc-800 flex items-center justify-center">
                    <span className="text-2xl">⌘</span>
                </div>
                <div className="text-center">
                    <p className="text-base font-medium text-zinc-400">Claude Orchestrator</p>
                    <p className="text-sm text-zinc-600 mt-1">Select a session or create a new one.</p>
                </div>
            </div>
        </>
    );
}

// ── Routes ────────────────────────────────────────────────────────────────────

const rootRoute = createRootRoute({ component: RootLayout });

const indexRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/',
    component: HomePage,
});

export const sessionRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/session/$id',
    component: SessionViewer,
});

const routeTree = rootRoute.addChildren([indexRoute, sessionRoute]);

export const router = createRouter({ routeTree });

declare module '@tanstack/react-router' {
    interface Register {
        router: typeof router;
    }
}
