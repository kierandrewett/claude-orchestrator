import { useRouterState } from '@tanstack/react-router';
import { Search, Menu, Wifi, WifiOff } from 'lucide-react';
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
    onMobileMenu?: () => void;
}

export function Header({ onCommandPalette, onMobileMenu }: HeaderProps) {
    const routerState = useRouterState();
    const pathname = routerState.location.pathname;
    const metricsQuery = trpc.metrics.summary.useQuery(undefined, { refetchInterval: 5000 });
    const connected = metricsQuery.data?.connected ?? false;

    const title = Object.entries(PAGE_TITLES)
        .sort((a, b) => b[0].length - a[0].length)
        .find(([path]) => pathname === path || (path !== '/' && pathname.startsWith(path)))?.[1]
        ?? 'Claude Orchestrator';

    return (
        <header className="h-12 flex items-center px-3 border-b border-zinc-800/60 bg-zinc-950/95 backdrop-blur-sm shrink-0 gap-2 z-40">
            {/* Mobile menu button */}
            <button
                onClick={onMobileMenu}
                className="lg:hidden p-1.5 rounded-md text-zinc-500 hover:text-zinc-300 hover:bg-zinc-800/60 transition-colors"
                aria-label="Open navigation menu"
            >
                <Menu size={17} />
            </button>

            <span className="text-sm font-semibold text-zinc-200 truncate">{title}</span>

            <div className="flex-1" />

            {/* Command palette trigger */}
            <button
                onClick={onCommandPalette}
                className="hidden sm:flex items-center gap-2 px-2.5 py-1.5 rounded-lg bg-zinc-900 border border-zinc-800/80 text-xs text-zinc-500 hover:text-zinc-300 hover:bg-zinc-800/60 hover:border-zinc-700 transition-all"
            >
                <Search size={11} />
                <span>Search...</span>
                <kbd className="text-[10px] bg-zinc-800 border border-zinc-700/50 px-1.5 py-0.5 rounded font-mono text-zinc-500">⌘K</kbd>
            </button>

            {/* Connection indicator */}
            <div
                className={cn(
                    'flex items-center gap-1.5 px-2 py-1 rounded-full text-xs font-medium border transition-all',
                    connected
                        ? 'bg-emerald-500/8 text-emerald-400 border-emerald-500/20'
                        : 'bg-zinc-900 text-zinc-500 border-zinc-800/80',
                )}
            >
                {connected ? (
                    <Wifi size={11} className="shrink-0" />
                ) : (
                    <WifiOff size={11} className="shrink-0" />
                )}
                <span className="hidden sm:inline">{connected ? 'Live' : 'Offline'}</span>
            </div>
        </header>
    );
}
