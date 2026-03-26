import { CalendarClock, Trash2 } from 'lucide-react';
import { trpc } from '../../api/trpc';
import { Switch } from '../ui/switch';
import { Skeleton } from '../ui/skeleton';
import { formatDistanceToNow } from 'date-fns';
import { cn } from '../../lib/utils';

export function SchedulerPage() {
    const eventsQuery = trpc.scheduler.list.useQuery(undefined, { refetchInterval: 10_000 });
    const utils = trpc.useUtils();
    const events = eventsQuery.data?.events ?? [];

    const toggleMutation = trpc.scheduler.toggle.useMutation({
        onSuccess: () => utils.scheduler.list.invalidate(),
    });
    const deleteMutation = trpc.scheduler.delete.useMutation({
        onSuccess: () => utils.scheduler.list.invalidate(),
    });

    return (
        <div className="p-4 md:p-6 space-y-5 max-w-2xl">
            <div>
                <h1 className="text-sm font-semibold text-zinc-100">Scheduled Events</h1>
                <p className="text-xs text-zinc-600 mt-0.5">{events.length} configured</p>
            </div>

            {eventsQuery.isLoading ? (
                <div className="space-y-2">
                    {[...Array(3)].map((_, i) => <Skeleton key={i} className="h-20 rounded-xl" />)}
                </div>
            ) : events.length === 0 ? (
                <div className="flex flex-col items-center justify-center py-16 text-center">
                    <div className="w-12 h-12 rounded-xl bg-zinc-900 border border-zinc-800 flex items-center justify-center mb-4">
                        <CalendarClock size={20} className="text-zinc-700" />
                    </div>
                    <p className="text-sm font-medium text-zinc-500">No scheduled events</p>
                    <p className="text-xs text-zinc-700 mt-1 max-w-xs">
                        Scheduled events can be configured via the API or config file.
                    </p>
                </div>
            ) : (
                <div className="space-y-2">
                    {events.map(event => (
                        <div
                            key={event.id}
                            className={cn(
                                'rounded-xl border bg-zinc-900/50 overflow-hidden transition-opacity',
                                !event.enabled && 'opacity-60',
                                'border-zinc-800/80',
                            )}
                        >
                            <div className="px-4 py-3 flex items-start gap-3">
                                <div className="flex-1 min-w-0">
                                    <div className="flex items-center gap-2 flex-wrap">
                                        <span className="text-sm font-medium text-zinc-200">{event.name}</span>
                                        <span className={cn(
                                            'text-[9px] font-semibold px-1.5 py-0.5 rounded-full border uppercase tracking-wide',
                                            event.enabled
                                                ? 'bg-emerald-500/10 text-emerald-400 border-emerald-500/20'
                                                : 'bg-zinc-800 text-zinc-600 border-zinc-700/50',
                                        )}>
                                            {event.enabled ? 'Enabled' : 'Disabled'}
                                        </span>
                                    </div>
                                    <code className="text-[11px] font-mono text-zinc-600 mt-0.5 block">
                                        {event.cron}
                                    </code>
                                    {event.next_run && (
                                        <p className="text-xs text-zinc-700 mt-0.5">
                                            Next run {formatDistanceToNow(new Date(event.next_run), { addSuffix: true })}
                                        </p>
                                    )}
                                </div>
                                <div className="flex items-center gap-2 shrink-0">
                                    <Switch
                                        checked={event.enabled}
                                        onCheckedChange={enabled => toggleMutation.mutate({ id: event.id, enabled })}
                                        disabled={toggleMutation.isPending}
                                    />
                                    <button
                                        onClick={() => {
                                            if (confirm(`Delete "${event.name}"?`)) {
                                                deleteMutation.mutate(event.id);
                                            }
                                        }}
                                        className="p-1.5 rounded-md hover:bg-zinc-800 text-zinc-700 hover:text-red-400 transition-colors"
                                        title="Delete event"
                                    >
                                        <Trash2 size={13} />
                                    </button>
                                </div>
                            </div>
                            {event.prompt && (
                                <div className="px-4 py-2 border-t border-zinc-800/40">
                                    <p className="text-xs text-zinc-600 truncate">{event.prompt}</p>
                                </div>
                            )}
                        </div>
                    ))}
                </div>
            )}
        </div>
    );
}
