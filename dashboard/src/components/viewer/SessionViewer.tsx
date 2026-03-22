import { useNavigate } from '@tanstack/react-router';
import { ArrowLeft, FolderOpen, Square } from 'lucide-react';
import { useQuery, useMutation } from '@tanstack/react-query';
import { fetchSessions, fetchHistory, killSession } from '../../api/client';
import { sessionRoute } from '../../router';
import { cn, getStatusBgColor, getStatusDot } from '../../lib/utils';
import { StatsPanel } from './StatsPanel';
import { EventStream } from './EventStream';
import { InputBar } from './InputBar';
import type { SessionInfo, ClaudeEvent } from '../../types';

export function SessionViewer() {
    const { id } = sessionRoute.useParams();
    const navigate = useNavigate();

    const { data: sessionsData } = useQuery({
        queryKey: ['sessions'],
        queryFn: fetchSessions,
        staleTime: 0,
    });
    const session: SessionInfo | undefined = sessionsData?.sessions.find((s) => s.id === id);

    // Fetch history — staleTime: Infinity means we use the cached version if available.
    // SSE appends new events to the cache so we never lose messages on re-navigation.
    const { data: historyData } = useQuery({
        queryKey: ['history', id],
        queryFn: () => fetchHistory(id),
        staleTime: Infinity,
        enabled: !!id,
    });
    const events: ClaudeEvent[] = historyData?.events ?? [];

    const killMutation = useMutation({ mutationFn: () => killSession(id) });

    if (!session) {
        return (
            <div className="flex flex-col items-center justify-center h-full gap-3 text-zinc-600">
                <p className="text-sm">Session not found.</p>
                <button
                    onClick={() => void navigate({ to: '/' })}
                    className="text-sm text-zinc-500 hover:text-zinc-300 flex items-center gap-1.5 transition-colors"
                >
                    <ArrowLeft size={14} /> Back
                </button>
            </div>
        );
    }

    const displayName =
        session.name || session.cwd.split('/').filter(Boolean).pop() || session.id.slice(0, 8);

    const isRunning = session.status === 'running';
    const isPending = session.status === 'pending';

    const handleKill = () => {
        if (window.confirm(`Stop session "${displayName}"?`)) {
            killMutation.mutate();
        }
    };

    return (
        <div className="flex flex-col h-full overflow-hidden">
            {/* Session header — sticky */}
            <div className="flex items-center gap-2 px-3 py-2.5 border-b border-zinc-800/80 bg-zinc-950 shrink-0">
                <button
                    onClick={() => void navigate({ to: '/' })}
                    className="lg:hidden p-1.5 rounded-lg text-zinc-500 hover:text-zinc-200 hover:bg-zinc-800 transition-colors"
                >
                    <ArrowLeft size={16} />
                </button>

                <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2 min-w-0">
                        <span
                            className={cn(
                                'w-2 h-2 rounded-full shrink-0',
                                getStatusDot(session.status),
                                isRunning && 'animate-pulse-dot',
                            )}
                        />
                        <h1 className="text-sm font-semibold text-zinc-100 truncate">
                            {displayName}
                        </h1>
                        <span
                            className={cn(
                                'text-[10px] font-medium px-1.5 py-0.5 rounded-full ring-1 ring-inset shrink-0',
                                getStatusBgColor(session.status),
                            )}
                        >
                            {session.status}
                        </span>
                    </div>

                    {session.cwd && (
                        <div className="flex items-center gap-1 mt-0.5 ml-4">
                            <FolderOpen size={10} className="text-zinc-700" />
                            <span className="text-[11px] text-zinc-600 font-mono truncate">
                                {session.cwd}
                            </span>
                        </div>
                    )}
                </div>

                {isRunning && (
                    <button
                        onClick={handleKill}
                        className="flex items-center gap-1 px-2.5 py-1.5 text-xs rounded-lg border border-red-900/30 bg-red-950/20 text-red-500 hover:bg-red-950/40 transition-colors shrink-0"
                    >
                        <Square size={11} />
                        Stop
                    </button>
                )}
            </div>

            <StatsPanel session={session} />
            <EventStream events={events} />

            {(isPending || isRunning) && (
                <InputBar sessionId={session.id} onKill={handleKill} pending={isPending} />
            )}
        </div>
    );
}
