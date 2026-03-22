import { Terminal, Menu } from 'lucide-react';
import { useSessionsStore } from '../../store/sessions';
import { cn } from '../../lib/utils';

interface HeaderProps {
    onMenuClick: () => void;
}

export function Header({ onMenuClick }: HeaderProps) {
    const wsConnected = useSessionsStore((s) => s.wsConnected);
    const clientStatus = useSessionsStore((s) => s.clientStatus);

    return (
        <header className="h-12 flex items-center px-4 border-b border-zinc-800 bg-zinc-900 shrink-0 gap-3 z-10">
            {/* Mobile menu button */}
            <button
                onClick={onMenuClick}
                className="lg:hidden p-1 rounded text-zinc-400 hover:text-zinc-100 hover:bg-zinc-800 transition-colors"
                aria-label="Toggle sidebar"
            >
                <Menu size={18} />
            </button>

            {/* Logo + title */}
            <div className="flex items-center gap-2 min-w-0">
                <Terminal size={18} className="text-emerald-400 shrink-0" />
                <span className="font-semibold text-sm tracking-tight text-zinc-100 whitespace-nowrap">
                    Claude Orchestrator
                </span>
            </div>

            <div className="flex-1" />

            {/* Client status badge */}
            <div className="flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-zinc-800 text-xs">
                <span
                    className={cn(
                        'w-1.5 h-1.5 rounded-full shrink-0',
                        clientStatus.connected ? 'bg-emerald-400 animate-pulse-dot' : 'bg-red-400',
                    )}
                />
                <span className={cn(clientStatus.connected ? 'text-emerald-400' : 'text-red-400')}>
                    {clientStatus.connected && clientStatus.hostname
                        ? clientStatus.hostname
                        : clientStatus.connected
                          ? 'Client'
                          : 'No Client'}
                </span>
            </div>

            {/* WS connection indicator */}
            <div className="flex items-center gap-1.5 px-2 py-1 rounded-full bg-zinc-800 text-xs">
                <span
                    className={cn(
                        'w-1.5 h-1.5 rounded-full shrink-0',
                        wsConnected ? 'bg-emerald-400' : 'bg-zinc-600',
                    )}
                    title={wsConnected ? 'WebSocket connected' : 'WebSocket disconnected'}
                />
                <span className="text-zinc-400 hidden sm:inline">
                    {wsConnected ? 'WS' : 'Offline'}
                </span>
            </div>
        </header>
    );
}
