import {
    createRouter,
    createRoute,
    createRootRoute,
    Outlet,
    useParams,
    useNavigate,
} from '@tanstack/react-router';
import { List, MessageSquare } from 'lucide-react';
import { useSSE } from './hooks/useSSE';
import { Header } from './components/layout/Header';
import { Sidebar } from './components/layout/Sidebar';
import { SessionViewer } from './components/viewer/SessionViewer';
import { cn } from './lib/utils';

// ── Mobile bottom nav ─────────────────────────────────────────────────────────

function MobileNav() {
    const params = useParams({ strict: false }) as Record<string, string>;
    const activeId = params.id;
    const navigate = useNavigate();
    const onSession = !!activeId;

    return (
        <nav className="lg:hidden flex shrink-0 border-t border-zinc-800 bg-zinc-950">
            <button
                onClick={() => void navigate({ to: '/' })}
                className={cn(
                    'flex-1 flex flex-col items-center gap-1 py-3 text-[11px] font-medium transition-colors',
                    !onSession ? 'text-zinc-200' : 'text-zinc-500 hover:text-zinc-400',
                )}
            >
                <List size={19} />
                Sessions
            </button>
            <button
                onClick={() =>
                    activeId &&
                    void navigate({ to: '/session/$id', params: { id: activeId } })
                }
                disabled={!activeId}
                className={cn(
                    'flex-1 flex flex-col items-center gap-1 py-3 text-[11px] font-medium transition-colors',
                    onSession ? 'text-zinc-200' : 'text-zinc-500',
                    !activeId && 'opacity-30 cursor-not-allowed',
                )}
            >
                <MessageSquare size={19} />
                Chat
            </button>
        </nav>
    );
}

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

            <MobileNav />
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
