import { Link, useRouterState } from '@tanstack/react-router';
import {
    LayoutDashboard,
    Terminal,
    Plug,
    Clock,
    Settings2,
    Bot,
} from 'lucide-react';
import { cn } from '../../lib/utils';
import { trpc } from '../../api/trpc';

const navItems = [
    { to: '/', label: 'Overview', icon: LayoutDashboard, exact: true },
    { to: '/tasks', label: 'Tasks', icon: Terminal },
    { to: '/mcp', label: 'MCP Servers', icon: Plug },
    { to: '/scheduler', label: 'Scheduled Events', icon: Clock },
    { to: '/config', label: 'Configuration', icon: Settings2 },
];

interface SidebarProps {
    onNavigate?: () => void;
}

export function Sidebar({ onNavigate }: SidebarProps) {
    const routerState = useRouterState();
    const pathname = routerState.location.pathname;

    const metricsQuery = trpc.metrics.summary.useQuery(undefined, {
        refetchInterval: 5000,
    });
    const metrics = metricsQuery.data;

    const runningCount = metrics?.running_tasks ?? 0;
    const connected = metrics?.connected ?? false;

    return (
        <div className="flex flex-col h-full overflow-hidden bg-zinc-900">
            {/* Logo */}
            <div className="flex items-center gap-2.5 px-4 py-4 border-b border-zinc-800 shrink-0">
                <div className="w-7 h-7 rounded-lg bg-zinc-700 flex items-center justify-center shrink-0">
                    <Bot size={15} className="text-zinc-300" />
                </div>
                <div className="min-w-0">
                    <p className="text-sm font-semibold text-zinc-100 truncate">Claude Orchestrator</p>
                </div>
            </div>

            {/* Navigation */}
            <nav className="flex-1 overflow-y-auto py-2">
                {navItems.map(({ to, label, icon: Icon, exact }) => {
                    const isActive = exact
                        ? pathname === to
                        : pathname.startsWith(to) && to !== '/';
                    return (
                        <Link
                            key={to}
                            to={to}
                            onClick={onNavigate}
                            className={cn(
                                'flex items-center gap-2.5 mx-2 px-3 py-2 rounded-lg text-sm font-medium transition-colors',
                                isActive
                                    ? 'bg-zinc-800 text-zinc-100'
                                    : 'text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800/50',
                            )}
                        >
                            <Icon size={16} className="shrink-0" />
                            <span className="flex-1 truncate">{label}</span>
                            {label === 'Tasks' && runningCount > 0 && (
                                <span className="text-[10px] font-medium bg-emerald-500/20 text-emerald-400 border border-emerald-500/30 px-1.5 py-0.5 rounded-full">
                                    {runningCount}
                                </span>
                            )}
                        </Link>
                    );
                })}
            </nav>

            {/* Connection status */}
            <div className="px-4 py-3 border-t border-zinc-800 shrink-0">
                <div className="flex items-center gap-2">
                    <span
                        className={cn(
                            'w-2 h-2 rounded-full shrink-0',
                            connected ? 'bg-emerald-400 animate-pulse' : 'bg-zinc-600',
                        )}
                    />
                    <span className="text-xs text-zinc-500">
                        {connected ? 'Connected' : 'Disconnected'}
                    </span>
                </div>
            </div>
        </div>
    );
}
