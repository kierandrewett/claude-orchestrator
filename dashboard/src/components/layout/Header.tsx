import { useRouterState } from '@tanstack/react-router';
import { Search } from 'lucide-react';
import { trpc } from '../../api/trpc';
import { cn } from '../../lib/utils';

const PAGE_TITLES: Record<string, string> = {
    '/': 'Overview',
    '/tasks': 'Tasks',
    '/mcp': 'MCP Servers',
    '/scheduler': 'Scheduled Events',
    '/config': 'Configuration',
};

interface HeaderProps {
    onCommandPalette?: () => void;
}

export function Header({ onCommandPalette }: HeaderProps) {
    const routerState = useRouterState();
    const pathname = routerState.location.pathname;
    const metricsQuery = trpc.metrics.summary.useQuery(undefined, { refetchInterval: 5000 });
    const connected = metricsQuery.data?.connected ?? false;

    const title = Object.entries(PAGE_TITLES)
        .sort((a, b) => b[0].length - a[0].length)
        .find(([path]) => pathname === path || (path !== '/' && pathname.startsWith(path)))?.[1]
        ?? 'Claude Orchestrator';

    return (
        <header className="h-12 flex items-center px-4 border-b border-zinc-800/80 bg-zinc-950 shrink-0 gap-3 z-40">
            <span className="text-sm font-semibold text-zinc-200">{title}</span>

            <div className="flex-1" />

            {/* Command palette trigger */}
            <button
                onClick={onCommandPalette}
                className="hidden sm:flex items-center gap-2 px-2.5 py-1.5 rounded-lg bg-zinc-800/60 border border-zinc-700/60 text-xs text-zinc-500 hover:text-zinc-300 hover:bg-zinc-800 transition-colors"
            >
                <Search size={12} />
                <span>Search...</span>
                <kbd className="text-[10px] bg-zinc-700/60 px-1 py-0.5 rounded">⌘K</kbd>
            </button>

            {/* Connection indicator */}
            <div className={cn(
                'flex items-center gap-1.5 px-2 py-1 rounded-full text-xs',
                connected ? 'bg-emerald-500/10 text-emerald-400' : 'bg-zinc-800 text-zinc-500',
            )}>
                <span className={cn(
                    'w-1.5 h-1.5 rounded-full',
                    connected ? 'bg-emerald-400 animate-pulse' : 'bg-zinc-600',
                )} />
                <span className="hidden sm:inline">{connected ? 'Connected' : 'Offline'}</span>
            </div>
        </header>
    );
}
