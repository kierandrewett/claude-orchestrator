import { useNavigate } from '@tanstack/react-router';
import { Moon, Square, MessageCircle } from 'lucide-react';
import { formatDistanceToNow } from 'date-fns';
import { cn, formatCost, formatTokens, getStatusDot } from '../../lib/utils';
import { Badge } from '../ui/badge';
import { trpc } from '../../api/trpc';
import type { TaskInfo } from '../../server-types';

interface TaskCardProps {
    task: TaskInfo;
    view?: 'grid' | 'list';
}

export function TaskCard({ task, view = 'grid' }: TaskCardProps) {
    const navigate = useNavigate();
    const utils = trpc.useUtils();
    const stopMutation = trpc.tasks.stop.useMutation({
        onSuccess: () => utils.tasks.list.invalidate(),
    });
    const hibernateMutation = trpc.tasks.hibernate.useMutation({
        onSuccess: () => utils.tasks.list.invalidate(),
    });

    const stateVariant =
        task.state === 'Running' ? 'success' :
        task.state === 'Hibernated' ? 'warning' : 'outline';

    if (view === 'list') {
        return (
            <div
                className="flex items-center gap-3 px-4 py-3 border-b border-zinc-800/60 last:border-0 cursor-pointer hover:bg-zinc-800/30 transition-colors group"
                onClick={() => void navigate({ to: '/tasks/$id', params: { id: task.id } })}
            >
                <span className={cn('w-2 h-2 rounded-full shrink-0', getStatusDot(task.state), task.state === 'Running' && 'animate-pulse')} />
                <span className="text-sm font-medium text-zinc-200 flex-1 truncate">{task.name}</span>
                <span className="text-xs text-zinc-500 font-mono shrink-0">{task.profile}</span>
                <Badge variant={stateVariant} className="shrink-0">{task.state}</Badge>
                <span className="text-xs text-zinc-500 shrink-0 tabular-nums">{formatCost(task.cost_usd)}</span>
                <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity shrink-0">
                    {task.state === 'Running' && (
                        <button
                            className="p-1.5 rounded hover:bg-zinc-700 text-zinc-500 hover:text-amber-400 transition-colors"
                            onClick={e => { e.stopPropagation(); hibernateMutation.mutate(task.id); }}
                            title="Hibernate"
                        >
                            <Moon size={12} />
                        </button>
                    )}
                    {task.state !== 'Dead' && (
                        <button
                            className="p-1.5 rounded hover:bg-zinc-700 text-zinc-500 hover:text-red-400 transition-colors"
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
            className="rounded-xl border border-zinc-800 bg-zinc-900 p-4 cursor-pointer hover:border-zinc-700 hover:bg-zinc-800/50 transition-all group"
            onClick={() => void navigate({ to: '/tasks/$id', params: { id: task.id } })}
        >
            <div className="flex items-start gap-2 mb-2">
                <span className={cn('w-2 h-2 rounded-full mt-1.5 shrink-0', getStatusDot(task.state), task.state === 'Running' && 'animate-pulse')} />
                <div className="flex-1 min-w-0">
                    <p className="text-sm font-medium text-zinc-200 truncate">{task.name}</p>
                    <p className="text-xs text-zinc-500 font-mono mt-0.5">{task.profile}</p>
                </div>
                <Badge variant={stateVariant}>{task.state}</Badge>
            </div>

            <div className="flex items-center gap-3 mt-3 text-xs text-zinc-500">
                <span className="tabular-nums">{formatCost(task.cost_usd)}</span>
                <span className="tabular-nums">{formatTokens(task.input_tokens + task.output_tokens)} tok</span>
                <span className="ml-auto">
                    {task.last_activity
                        ? formatDistanceToNow(new Date(task.last_activity), { addSuffix: true })
                        : '—'}
                </span>
            </div>

            {/* Hover actions */}
            <div className="flex items-center gap-1 mt-3 opacity-0 group-hover:opacity-100 transition-opacity">
                {task.state === 'Running' && (
                    <button
                        className="flex items-center gap-1 px-2 py-1 rounded text-xs text-zinc-400 hover:text-amber-400 hover:bg-zinc-800 transition-colors"
                        onClick={e => { e.stopPropagation(); hibernateMutation.mutate(task.id); }}
                    >
                        <Moon size={11} />
                        Hibernate
                    </button>
                )}
                {task.state !== 'Dead' && (
                    <button
                        className="flex items-center gap-1 px-2 py-1 rounded text-xs text-zinc-400 hover:text-red-400 hover:bg-zinc-800 transition-colors"
                        onClick={e => { e.stopPropagation(); stopMutation.mutate(task.id); }}
                    >
                        <Square size={11} />
                        Stop
                    </button>
                )}
                <button
                    className="flex items-center gap-1 px-2 py-1 rounded text-xs text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800 transition-colors ml-auto"
                    onClick={e => { e.stopPropagation(); void navigate({ to: '/tasks/$id', params: { id: task.id } }); }}
                >
                    <MessageCircle size={11} />
                    Open
                </button>
            </div>
        </div>
    );
}
