import * as React from 'react';
import { Outlet, useNavigate, useRouterState } from '@tanstack/react-router';
import { Header } from './Header';
import { Sidebar } from './Sidebar';
import { CommandPalette } from '../CommandPalette';
import { useSSE } from '../../hooks/useSSE';
import { ToastProvider, ToastViewport } from '../ui/toast';

export function AppShell() {
    const [cmdOpen, setCmdOpen] = React.useState(false);
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

    // Auth check - if server requires auth and we don't have a token, redirect to login
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
            .catch(() => {/* server may not be available yet */});
    }, [navigate, routerState.location.pathname]);

    return (
        <ToastProvider>
            <div
                className="flex flex-col bg-zinc-950 text-zinc-100 overflow-hidden"
                style={{ height: '100dvh' }}
            >
                <Header onCommandPalette={() => setCmdOpen(true)} />
                <div className="flex flex-1 overflow-hidden min-h-0">
                    {/* Sidebar — desktop only */}
                    <aside className="hidden lg:flex w-64 flex-col shrink-0 border-r border-zinc-800">
                        <Sidebar />
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
