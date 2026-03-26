import * as React from 'react';
import { Outlet, useNavigate, useRouterState } from '@tanstack/react-router';
import { Header } from './Header';
import { Sidebar } from './Sidebar';
import { CommandPalette } from '../CommandPalette';
import { useSSE } from '../../hooks/useSSE';
import { ToastProvider, ToastViewport } from '../ui/toast';
import { cn } from '../../lib/utils';

export function AppShell() {
    const [cmdOpen, setCmdOpen] = React.useState(false);
    const [mobileSidebarOpen, setMobileSidebarOpen] = React.useState(false);
    const navigate = useNavigate();
    const routerState = useRouterState();

    useSSE();

    // Cmd+K global shortcut
    React.useEffect(() => {
        const listener = (e: KeyboardEvent) => {
            if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
                e.preventDefault();
                setCmdOpen(v => !v);
            }
        };
        window.addEventListener('keydown', listener);
        return () => window.removeEventListener('keydown', listener);
    }, []);

    // Auth check
    React.useEffect(() => {
        const isLoginPage = routerState.location.pathname === '/login';
        if (isLoginPage) return;

        fetch('/api/health')
            .then(res => res.json() as Promise<{ auth_required: boolean }>)
            .then(data => {
                if (data.auth_required) {
                    const token = localStorage.getItem('dashboard_token');
                    if (!token) {
                        void navigate({ to: '/login' });
                    }
                }
            })
            .catch(() => {});
    }, [navigate, routerState.location.pathname]);

    // Close mobile sidebar on navigation
    React.useEffect(() => {
        setMobileSidebarOpen(false);
    }, [routerState.location.pathname]);

    return (
        <ToastProvider>
            <div
                className="flex flex-col bg-zinc-950 text-zinc-100 overflow-hidden"
                style={{ height: '100dvh' }}
            >
                <Header
                    onCommandPalette={() => setCmdOpen(true)}
                    onMobileMenu={() => setMobileSidebarOpen(v => !v)}
                />
                <div className="flex flex-1 overflow-hidden min-h-0">
                    {/* Desktop sidebar */}
                    <aside className="hidden lg:flex w-56 xl:w-60 flex-col shrink-0 border-r border-zinc-800/60">
                        <Sidebar />
                    </aside>

                    {/* Mobile sidebar backdrop */}
                    <div
                        className={cn(
                            'fixed inset-0 z-40 lg:hidden transition-opacity duration-200',
                            mobileSidebarOpen
                                ? 'opacity-100 pointer-events-auto'
                                : 'opacity-0 pointer-events-none',
                        )}
                        onClick={() => setMobileSidebarOpen(false)}
                    >
                        <div className="absolute inset-0 bg-black/70 backdrop-blur-sm" />
                    </div>

                    {/* Mobile sidebar drawer */}
                    <aside
                        className={cn(
                            'fixed inset-y-0 left-0 z-50 w-64 flex flex-col border-r border-zinc-800/60 shadow-2xl transition-transform duration-200 ease-out lg:hidden',
                            mobileSidebarOpen ? 'translate-x-0' : '-translate-x-full',
                        )}
                    >
                        <Sidebar onNavigate={() => setMobileSidebarOpen(false)} />
                    </aside>

                    <main className="flex-1 overflow-auto flex flex-col min-h-0">
                        <Outlet />
                    </main>
                </div>
            </div>

            <CommandPalette open={cmdOpen} onOpenChange={setCmdOpen} />
            <ToastViewport />
        </ToastProvider>
    );
}
