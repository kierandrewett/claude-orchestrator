import { Terminal, Wifi, WifiOff } from 'lucide-react';
import { useQuery } from '@tanstack/react-query';
import { fetchStatus } from '../../api/client';
import { cn } from '../../lib/utils';

export function Header() {
    const { data: status } = useQuery({
        queryKey: ['status'],
        queryFn: fetchStatus,
        staleTime: 0,
    });

    const clientConnected = status?.connected ?? false;
    const hostname = status?.hostname ?? null;

    return (
        <header className="h-12 flex items-center px-4 border-b border-zinc-800/80 bg-zinc-950 shrink-0 gap-3 z-40">
            <div className="flex items-center gap-2 min-w-0">
                <Terminal size={15} className="text-zinc-400 shrink-0" />
                <span className="font-medium text-sm text-zinc-300 whitespace-nowrap tracking-tight">
                    Claude Orchestrator
                </span>
            </div>

            <div className="flex-1" />

            <div
                className={cn(
                    'flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs transition-colors',
                    clientConnected
                        ? 'bg-emerald-500/10 text-emerald-400'
                        : 'bg-zinc-800 text-zinc-500',
                )}
            >
                {clientConnected ? (
                    <Wifi size={11} className="shrink-0" />
                ) : (
                    <WifiOff size={11} className="shrink-0" />
                )}
                <span className="hidden sm:inline">
                    {clientConnected && hostname
                        ? hostname
                        : clientConnected
                          ? 'Connected'
                          : 'No client'}
                </span>
            </div>
        </header>
    );
}
