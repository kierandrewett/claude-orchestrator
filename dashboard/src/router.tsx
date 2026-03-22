import { createRouter, createRoute, createRootRoute, Outlet } from '@tanstack/react-router';
import { useState } from 'react';
import { useSSE } from './hooks/useSSE';
import { Header } from './components/layout/Header';
import { Sidebar } from './components/layout/Sidebar';
import { SessionViewer } from './components/viewer/SessionViewer';

// ── Root layout ──────────────────────────────────────────────────────────────

function RootLayout() {
    useSSE();
    const [sidebarOpen, setSidebarOpen] = useState(false);

    return (
        <div className="flex flex-col h-screen bg-zinc-950 text-zinc-100 overflow-hidden">
            <Header onMenuClick={() => setSidebarOpen((v) => !v)} />
            <div className="flex flex-1 overflow-hidden">
                {/* Mobile backdrop */}
                {sidebarOpen && (
                    <div
                        className="fixed inset-0 z-20 bg-black/60 lg:hidden"
                        onClick={() => setSidebarOpen(false)}
                    />
                )}

                {/* Sidebar */}
                <aside
                    className={[
                        'fixed lg:static inset-y-0 left-0 z-30 w-72 bg-zinc-900 border-r border-zinc-800',
                        'transform transition-transform duration-200 ease-in-out',
                        'lg:transform-none lg:translate-x-0',
                        sidebarOpen ? 'translate-x-0' : '-translate-x-full',
                        'flex flex-col top-12 lg:top-0',
                    ].join(' ')}
                >
                    <Sidebar onNavigate={() => setSidebarOpen(false)} />
                </aside>

                <main className="flex-1 overflow-hidden flex flex-col">
                    <Outlet />
                </main>
            </div>
        </div>
    );
}

function HomePage() {
    return (
        <div className="flex flex-col items-center justify-center h-full text-zinc-500 gap-4">
            <div className="w-12 h-12 rounded-full bg-zinc-800 flex items-center justify-center">
                <span className="text-2xl">⌘</span>
            </div>
            <div className="text-center">
                <p className="text-base font-medium text-zinc-400">Claude Orchestrator</p>
                <p className="text-sm text-zinc-600 mt-1">Select a session or create a new one.</p>
            </div>
        </div>
    );
}

// ── Routes ───────────────────────────────────────────────────────────────────

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
