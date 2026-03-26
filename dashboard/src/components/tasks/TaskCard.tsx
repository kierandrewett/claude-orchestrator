import { useNavigate } from '@tanstack/react-router';
import { Moon, Square } from 'lucide-react';
import { formatDistanceToNow } from 'date-fns';
import { cn, formatCost, formatTokens, getStatusDot } from '../../lib/utils';
import { trpc } from '../../api/trpc';
import type { TaskInfo } from '../../server-types';

interface TaskCardProps {
    task: TaskInfo;
    view?: 'grid' | 'list';
    isLast?: boolean;
}

export function TaskCard({ task, view = 'grid', isLast }: TaskCardProps) {
    const navigate = useNavigate();
    const utils = trpc.useUtils();
    const stopMutation = trpc.tasks.stop.useMutation({
        onSuccess: () => utils.tasks.list.invalidate(),
    });
    const hibernateMutation = trpc.tasks.hibernate.useMutation({
        onSuccess: () => utils.tasks.list.invalidate(),
    });

    const isRunning = task.state === 'Running';
    const isHibernated = task.state === 'Hibernated';

    const lastActivity = task.last_activity
        ? formatDistanceToNow(new Date(task.last_activity), { addSuffix: true })
        : null;

    if (view === 'list') {
        return (
            <div
                className={cn(
                    'flex items-center gap-3 px-4 py-2.5 cursor-pointer hover:bg-zinc-800/30 transition-colors group',
                    !isLast && 'border-b border-zinc-800/40',
                )}
                onClick={() => void navigate({ to: '/tasks/$id', params: { id: task.id } })}
            >
                <span
                    className={cn(
                        'w-1.5 h-1.5 rounded-full shrink-0',
                        getStatusDot(task.state),
                        isRunning && 'animate-pulse-dot',
                    )}
                />
                <span className="text-sm font-medium text-zinc-200 flex-1 truncate min-w-0">{task.name}</span>
                <span className="text-xs text-zinc-600 font-mono shrink-0 hidden sm:block">{task.profile}</span>
                <span className={cn(
                    'text-[10px] font-semibold px-1.5 py-0.5 rounded-full border shrink-0',
                    isRunning    ? 'bg-emerald-500/10 text-emerald-400 border-emerald-500/20' :
                    isHibernated ? 'bg-amber-500/10 text-amber-400 border-amber-500/20' :
                                   'bg-zinc-800 text-zinc-500 border-zinc-700/50',
                )}>
                    {task.state}
                </span>
                <span className="text-xs text-zinc-600 tabular-nums shrink-0 hidden md:block">{formatCost(task.cost_usd)}</span>
                {/* Hover actions */}
                <div className="flex items-center gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity shrink-0">
                    {isRunning && (
                        <button
                            className="p-1.5 rounded-md hover:bg-zinc-700/60 text-zinc-600 hover:text-amber-400 transition-colors"
                            onClick={e => { e.stopPropagation(); hibernateMutation.mutate(task.id); }}
                            title="Hibernate"
                        >
                            <Moon size={12} />
                        </button>
                    )}
                    {!task.state.includes('Dead') && (
                        <button
                            className="p-1.5 rounded-md hover:bg-zinc-700/60 text-zinc-600 hover:text-red-400 transition-colors"
                            onClick={e => { e.stopPropagation(); stopMutation.mutate(task.id); }}
                            title="Stop"
                        >
                            <Square size={12} />
                        </button>
                    )}
                </div>
            </div>
        );
    }

    return (
        <div
            className="group rounded-xl border border-zinc-800/80 bg-zinc-900/50 p-4 cursor-pointer hover:border-zinc-700 hover:bg-zinc-900 transition-all flex flex-col gap-3"
            onClick={() => void navigate({ to: '/tasks/$id', params: { id: task.id } })}
        >
            {/* Header */}
            <div className="flex items-start gap-2 min-w-0">
                <span
                    className={cn(
                        'w-1.5 h-1.5 rounded-full mt-1.5 shrink-0',
                        getStatusDot(task.state),
                        isRunning && 'animate-pulse-dot',
                    )}
                />
                <div className="flex-1 min-w-0">
                    <p className="text-sm font-medium text-zinc-200 truncate leading-tight">{task.name}</p>
                    <p className="text-[11px] text-zinc-600 font-mono mt-0.5 truncate">{task.profile}</p>
                </div>
                <span className={cn(
                    'text-[10px] font-semibold px-1.5 py-0.5 rounded-full border shrink-0',
                    isRunning    ? 'bg-emerald-500/10 text-emerald-400 border-emerald-500/20' :
                    isHibernated ? 'bg-amber-500/10 text-amber-400 border-amber-500/20' :
                                   'bg-zinc-800/80 text-zinc-500 border-zinc-700/40',
                )}>
                    {task.state}
                </span>
            </div>

            {/* Stats */}
            <div className="flex items-center gap-3 text-xs text-zinc-600 border-t border-zinc-800/40 pt-2.5">
                <span className="tabular-nums">{formatCost(task.cost_usd)}</span>
                <span className="tabular-nums">{formatTokens(task.input_tokens + task.output_tokens)}t</span>
                {task.turns > 0 && <span className="tabular-nums">{task.turns} turns</span>}
                {lastActivity && (
                    <span className="ml-auto truncate">{lastActivity}</span>
                )}
            </div>

            {/* Hover actions */}
            <div className="flex items-center gap-1 -mt-1 opacity-0 group-hover:opacity-100 transition-opacity">
                {isRunning && (
                    <button
                        className="flex items-center gap-1 px-2 py-1 rounded-md text-xs text-zinc-500 hover:text-amber-400 hover:bg-zinc-800/60 transition-colors"
                        onClick={e => { e.stopPropagation(); hibernateMutation.mutate(task.id); }}
                    >
                        <Moon size={11} /> Hibernate
                    </button>
                )}
                {task.state !== 'Dead' && (
                    <button
                        className="flex items-center gap-1 px-2 py-1 rounded-md text-xs text-zinc-500 hover:text-red-400 hover:bg-zinc-800/60 transition-colors"
                        onClick={e => { e.stopPropagation(); stopMutation.mutate(task.id); }}
                    >
                        <Square size={11} /> Stop
                    </button>
                )}
            </div>
        </div>
    );
}
