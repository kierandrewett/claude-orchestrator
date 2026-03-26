import { Link, useRouterState } from '@tanstack/react-router';
import {
    LayoutDashboard,
    Terminal,
    Plug,
    CalendarClock,
    Settings2,
    Bot,
    Wifi,
    WifiOff,
} from 'lucide-react';
import { cn } from '../../lib/utils';
import { trpc } from '../../api/trpc';

const navItems = [
    { to: '/', label: 'Overview', icon: LayoutDashboard, exact: true },
    { to: '/tasks', label: 'Tasks', icon: Terminal },
    { to: '/mcp', label: 'MCP Servers', icon: Plug },
    { to: '/scheduler', label: 'Scheduled', icon: CalendarClock },
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
        <div className="flex flex-col h-full overflow-hidden bg-zinc-950">
            {/* Logo */}
            <div className="flex items-center gap-2.5 px-4 py-[14px] border-b border-zinc-800/60 shrink-0">
                <div className="w-6 h-6 rounded-md bg-zinc-800 flex items-center justify-center shrink-0 border border-zinc-700/50">
                    <Bot size={13} className="text-zinc-300" />
                </div>
                <p className="text-[13px] font-semibold text-zinc-100 truncate leading-tight">
                    Claude Orchestrator
                </p>
            </div>

            {/* Navigation */}
            <nav className="flex-1 overflow-y-auto py-2 px-2" aria-label="Main navigation">
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
                                'group flex items-center gap-2.5 px-2.5 py-[7px] rounded-md text-[13px] font-medium transition-colors mb-0.5',
                                isActive
                                    ? 'bg-zinc-800/70 text-zinc-100'
                                    : 'text-zinc-500 hover:text-zinc-300 hover:bg-zinc-900',
                            )}
                        >
                            <Icon
                                size={14}
                                className={cn(
                                    'shrink-0 transition-colors',
                                    isActive ? 'text-zinc-300' : 'text-zinc-600 group-hover:text-zinc-400',
                                )}
                            />
                            <span className="flex-1 truncate">{label}</span>
                            {label === 'Tasks' && runningCount > 0 && (
                                <span className="text-[10px] font-semibold bg-emerald-500/15 text-emerald-400 border border-emerald-500/20 px-1.5 py-px rounded-full tabular-nums leading-tight">
                                    {runningCount}
                                </span>
                            )}
                        </Link>
                    );
                })}
            </nav>

            {/* Connection status footer */}
            <div className="px-3 py-3 border-t border-zinc-800/60 shrink-0">
                <div
                    className={cn(
                        'flex items-center gap-2 px-2 py-1.5 rounded-md text-[11px] font-medium',
                        connected ? 'text-emerald-400' : 'text-zinc-600',
                    )}
                >
                    {connected ? (
                        <Wifi size={11} className="shrink-0" />
                    ) : (
                        <WifiOff size={11} className="shrink-0" />
                    )}
                    <span>{connected ? 'Connected to orchestrator' : 'Disconnected'}</span>
                    {connected && (
                        <span className="ml-auto w-1.5 h-1.5 rounded-full bg-emerald-400 animate-pulse shrink-0" />
                    )}
                </div>
            </div>
        </div>
    );
}
