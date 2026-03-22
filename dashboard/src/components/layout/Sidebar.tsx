import { useState, useMemo, useEffect, useRef } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { Plus, FolderOpen } from 'lucide-react';
import { useSessionsStore } from '../../store/sessions';
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

    const displayName = session.name || session.cwd.split('/').filter(Boolean).pop() || session.cwd;

    return (
        <button
            onClick={onClick}
            className={cn(
                'w-full text-left px-3 py-2.5 rounded-lg transition-colors group',
                'flex flex-col gap-1 border',
                isActive
                    ? 'bg-zinc-800 border-zinc-700'
                    : 'bg-transparent border-transparent hover:bg-zinc-800/60 hover:border-zinc-700/50',
            )}
        >
            <div className="flex items-center gap-2 min-w-0">
                <span
                    className={cn(
                        'w-1.5 h-1.5 rounded-full shrink-0',
                        getStatusDot(session.status),
                        session.status === 'running' && 'animate-pulse-dot',
                    )}
                />
                <span className="text-sm font-medium text-zinc-200 truncate flex-1">
                    {displayName}
                </span>
                <span
                    className={cn(
                        'text-[10px] font-medium px-1.5 py-0.5 rounded ring-1 ring-inset shrink-0',
                        getStatusBgColor(session.status),
                    )}
                >
                    {session.status}
                </span>
            </div>

            <div className="flex items-center gap-1.5 ml-3.5">
                <FolderOpen size={11} className="text-zinc-600 shrink-0" />
                <span className="text-[11px] text-zinc-500 truncate font-mono">{session.cwd}</span>
            </div>

            <div className="ml-3.5 text-[11px] text-zinc-600">{duration}</div>
        </button>
    );
}

interface SidebarProps {
    onNavigate: () => void;
}

export function Sidebar({ onNavigate }: SidebarProps) {
    const { id: activeId } = useParams<{ id: string }>();
    const navigate = useNavigate();
    const sessions = useSessionsStore((s) => s.sessions);
    const createSession = useSessionsStore((s) => s.createSession);
    const wsConnected = useSessionsStore((s) => s.wsConnected);

    // Track how many sessions existed before we fired create, so we can
    // navigate to the newly-created one when session_created arrives.
    const prevCountRef = useRef<number | null>(null);

    const sortedSessions = useMemo(
        () =>
            Object.values(sessions).sort(
                (a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime(),
            ),
        [sessions],
    );

    useEffect(() => {
        if (prevCountRef.current === null) return;
        if (sortedSessions.length > prevCountRef.current) {
            prevCountRef.current = null;
            const newest = sortedSessions[0];
            if (newest) {
                navigate(`/session/${newest.id}`);
                onNavigate();
            }
        }
    }, [sortedSessions, navigate, onNavigate]);

    const handleNew = () => {
        if (!wsConnected) return;
        prevCountRef.current = sortedSessions.length;
        createSession({});
    };

    const handleSessionClick = (id: string) => {
        navigate(`/session/${id}`);
        onNavigate();
    };

    return (
        <div className="flex flex-col h-full overflow-hidden">
            {/* Header */}
            <div className="flex items-center justify-between px-4 py-3 border-b border-zinc-800 shrink-0">
                <span className="text-xs font-semibold text-zinc-400 uppercase tracking-wider">
                    Sessions
                </span>
                <button
                    onClick={handleNew}
                    disabled={!wsConnected}
                    className="flex items-center gap-1.5 px-2 py-1 text-xs rounded-md bg-emerald-500/10 text-emerald-400 hover:bg-emerald-500/20 transition-colors border border-emerald-500/20 hover:border-emerald-500/30 disabled:opacity-40 disabled:cursor-not-allowed"
                    title="New Session"
                >
                    <Plus size={13} />
                    New
                </button>
            </div>

            {/* Session list */}
            <div className="flex-1 overflow-y-auto p-2 flex flex-col gap-1">
                {sortedSessions.length === 0 ? (
                    <div className="flex flex-col items-center justify-center h-32 gap-2 text-zinc-600">
                        <span className="text-sm">No sessions yet</span>
                        <span className="text-xs">Create one to get started</span>
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
