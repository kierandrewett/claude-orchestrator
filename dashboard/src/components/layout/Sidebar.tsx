import { useMemo, useRef } from 'react';
import { useNavigate, useParams } from '@tanstack/react-router';
import { Plus, FolderOpen, Clock } from 'lucide-react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { fetchSessions, createSession, fetchStatus } from '../../api/client';
import { cn, getStatusDot, getStatusBgColor } from '../../lib/utils';
import { useLiveDuration } from '../../hooks/useLiveDuration';
import type { SessionInfo } from '../../types';

interface SessionCardProps {
    session: SessionInfo;
    isActive: boolean;
    onClick: () => void;
}

function SessionCard({ session, isActive, onClick }: SessionCardProps) {
    const duration = useLiveDuration(
        session.started_at,
        session.ended_at,
        session.status === 'running',
    );

    const displayName =
        session.name || session.cwd.split('/').filter(Boolean).pop() || session.id.slice(0, 8);

    return (
        <button
            onClick={onClick}
            className={cn(
                'w-full text-left px-4 py-3 transition-all',
                'border-b border-zinc-800/60 last:border-0',
                isActive ? 'bg-zinc-800' : 'hover:bg-zinc-800/50 active:bg-zinc-800',
            )}
        >
            <div className="flex items-center gap-2.5 min-w-0">
                <span
                    className={cn(
                        'w-2 h-2 rounded-full shrink-0',
                        getStatusDot(session.status),
                        session.status === 'running' && 'animate-pulse-dot',
                    )}
                />
                <span className="text-sm font-medium text-zinc-200 truncate flex-1">
                    {displayName}
                </span>
                <span
                    className={cn(
                        'text-[10px] font-medium px-1.5 py-0.5 rounded-full ring-1 ring-inset shrink-0',
                        getStatusBgColor(session.status),
                    )}
                >
                    {session.status}
                </span>
            </div>

            <div className="flex items-center gap-3 mt-1.5 ml-4.5">
                {session.cwd && (
                    <div className="flex items-center gap-1 min-w-0">
                        <FolderOpen size={10} className="text-zinc-600 shrink-0" />
                        <span className="text-[11px] text-zinc-500 truncate font-mono">
                            {session.cwd}
                        </span>
                    </div>
                )}
                <div className="flex items-center gap-1 shrink-0 ml-auto">
                    <Clock size={10} className="text-zinc-700" />
                    <span className="text-[11px] text-zinc-600">{duration}</span>
                </div>
            </div>
        </button>
    );
}

interface SidebarProps {
    onNavigate: () => void;
}

export function Sidebar({ onNavigate }: SidebarProps) {
    const params = useParams({ strict: false });
    const activeId = (params as Record<string, string>).id;
    const navigate = useNavigate();
    const queryClient = useQueryClient();
    const onNavigateRef = useRef(onNavigate);
    onNavigateRef.current = onNavigate;

    const { data: sessionsData } = useQuery({
        queryKey: ['sessions'],
        queryFn: fetchSessions,
        staleTime: 0,
    });

    const { data: status } = useQuery({
        queryKey: ['status'],
        queryFn: fetchStatus,
        staleTime: 0,
    });

    const clientConnected = status?.connected ?? false;

    const sortedSessions = useMemo(
        () =>
            [...(sessionsData?.sessions ?? [])].sort(
                (a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime(),
            ),
        [sessionsData],
    );

    const createMutation = useMutation({
        mutationFn: createSession,
        onSuccess: (data) => {
            queryClient.setQueryData<{ sessions: SessionInfo[] }>(['sessions'], (old) => ({
                sessions: [
                    data.session,
                    ...(old?.sessions ?? []).filter((s) => s.id !== data.session.id),
                ],
            }));
            void navigate({ to: '/session/$id', params: { id: data.session.id } });
            onNavigateRef.current();
        },
    });

    const handleNew = () => {
        if (!clientConnected || createMutation.isPending) return;
        createMutation.mutate({});
    };

    const handleSessionClick = (id: string) => {
        void navigate({ to: '/session/$id', params: { id } });
        onNavigate();
    };

    return (
        <div className="flex flex-col h-full overflow-hidden">
            {/* Header */}
            <div className="flex items-center justify-between px-4 py-3 border-b border-zinc-800 shrink-0 bg-zinc-900 sticky top-0 z-10">
                <span className="text-xs font-semibold text-zinc-400 uppercase tracking-wider">
                    Sessions
                </span>
                <button
                    onClick={handleNew}
                    disabled={!clientConnected || createMutation.isPending}
                    className="flex items-center gap-1.5 px-3 py-1.5 text-xs rounded-lg bg-zinc-800 text-zinc-300 hover:bg-zinc-700 hover:text-zinc-100 transition-colors border border-zinc-700/50 disabled:opacity-40 disabled:cursor-not-allowed font-medium"
                    title={clientConnected ? 'New session' : 'No client connected'}
                >
                    <Plus size={13} />
                    New
                </button>
            </div>

            {/* Session list */}
            <div className="flex-1 overflow-y-auto flex flex-col">
                {sortedSessions.length === 0 ? (
                    <div className="flex flex-col items-center justify-center flex-1 gap-2 text-zinc-600 py-16">
                        <span className="text-sm">No sessions yet</span>
                        <span className="text-xs text-zinc-700">
                            {clientConnected ? 'Tap New to get started' : 'Connect a client first'}
                        </span>
                    </div>
                ) : (
                    sortedSessions.map((session) => (
                        <SessionCard
                            key={session.id}
                            session={session}
                            isActive={session.id === activeId}
                            onClick={() => handleSessionClick(session.id)}
                        />
                    ))
                )}
            </div>
        </div>
    );
}
