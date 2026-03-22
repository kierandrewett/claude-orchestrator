import { useEffect } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { ArrowLeft, FolderOpen, Square } from 'lucide-react';
import { useSessionsStore } from '../../store/sessions';
import { cn, getStatusBgColor, getStatusDot } from '../../lib/utils';
import { StatsPanel } from './StatsPanel';
import { EventStream } from './EventStream';
import { InputBar } from './InputBar';

export function SessionViewer() {
    const { id } = useParams<{ id: string }>();
    const navigate = useNavigate();

    const session = useSessionsStore((s) => (id ? s.sessions[id] : undefined));
    const events = useSessionsStore((s) => (id ? (s.events[id] ?? []) : []));
    const requestHistory = useSessionsStore((s) => s.requestHistory);
    const killSession = useSessionsStore((s) => s.killSession);

    useEffect(() => {
        if (id) {
            requestHistory(id);
        }
    }, [id, requestHistory]);

    if (!id) {
        return (
            <div className="flex items-center justify-center h-full text-zinc-600">
                No session selected.
            </div>
        );
    }

    if (!session) {
        return (
            <div className="flex flex-col items-center justify-center h-full gap-3 text-zinc-600">
                <p>Session not found.</p>
                <button
                    onClick={() => navigate('/')}
                    className="text-sm text-zinc-500 hover:text-zinc-300 flex items-center gap-1.5"
                >
                    <ArrowLeft size={14} /> Back to sessions
                </button>
            </div>
        );
    }

    const displayName =
        session.name || session.cwd.split('/').filter(Boolean).pop() || session.id.slice(0, 8);

    const isRunning = session.status === 'running';
    const isPending = session.status === 'pending';

    const handleKill = () => {
        if (window.confirm(`Kill session "${displayName}"?`)) {
            killSession(session.id);
        }
    };

    return (
        <div className="flex flex-col h-full overflow-hidden">
            {/* Session header */}
            <div className="flex items-center gap-3 px-4 py-3 border-b border-zinc-800 bg-zinc-900/30 shrink-0">
                <button
                    onClick={() => navigate('/')}
                    className="lg:hidden p-1 rounded text-zinc-500 hover:text-zinc-300 hover:bg-zinc-800 transition-colors"
                >
                    <ArrowLeft size={16} />
                </button>

                <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2 min-w-0">
                        {/* Status dot */}
                        <span
                            className={cn(
                                'w-2 h-2 rounded-full shrink-0',
                                getStatusDot(session.status),
                                isRunning && 'animate-pulse-dot',
                            )}
                        />
                        {/* Name */}
                        <h1 className="text-sm font-semibold text-zinc-100 truncate">
                            {displayName}
                        </h1>
                        {/* Status badge */}
                        <span
                            className={cn(
                                'text-[10px] font-medium px-1.5 py-0.5 rounded ring-1 ring-inset shrink-0',
                                getStatusBgColor(session.status),
                            )}
                        >
                            {session.status}
                        </span>
                    </div>

                    {/* CWD */}
                    <div className="flex items-center gap-1.5 mt-0.5 ml-4">
                        <FolderOpen size={11} className="text-zinc-600" />
                        <span className="text-[11px] text-zinc-500 font-mono truncate">
                            {session.cwd}
                        </span>
                    </div>
                </div>

                {/* Kill button */}
                {isRunning && (
                    <button
                        onClick={handleKill}
                        className="flex items-center gap-1.5 px-3 py-1.5 text-xs rounded-lg border border-red-800/40 bg-red-900/10 text-red-500 hover:bg-red-900/20 hover:border-red-700/50 transition-colors shrink-0"
                    >
                        <Square size={13} />
                        Kill
                    </button>
                )}
            </div>

            {/* Stats panel */}
            <StatsPanel session={session} />

            {/* Event stream — takes remaining height */}
            <EventStream events={events} />

            {/* Input bar — shown while pending or running */}
            {(isPending || isRunning) && (
                <InputBar sessionId={session.id} onKill={handleKill} pending={isPending} />
            )}
        </div>
    );
}
