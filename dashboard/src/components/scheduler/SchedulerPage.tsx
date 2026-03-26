import { Clock, Trash2 } from 'lucide-react';
import { trpc } from '../../api/trpc';
import { Switch } from '../ui/switch';
import { Badge } from '../ui/badge';
import { Skeleton } from '../ui/skeleton';
import { formatDistanceToNow } from 'date-fns';

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
        <div className="p-6 space-y-6 max-w-3xl">
            <div className="flex items-center gap-3">
                <div className="flex-1">
                    <h1 className="text-lg font-semibold text-zinc-100">Scheduled Events</h1>
                    <p className="text-sm text-zinc-500">{events.length} configured</p>
                </div>
            </div>

            {eventsQuery.isLoading ? (
                <div className="space-y-3">
                    {[...Array(3)].map((_, i) => <Skeleton key={i} className="h-20 rounded-xl" />)}
                </div>
            ) : events.length === 0 ? (
                <div className="flex flex-col items-center justify-center py-16 text-zinc-600">
                    <Clock size={24} className="mb-3" />
                    <p className="text-sm">No scheduled events</p>
                    <p className="text-xs mt-1">Scheduled events can be configured via the API or config file</p>
                </div>
            ) : (
                <div className="space-y-3">
                    {events.map(event => (
                        <div key={event.id} className="rounded-xl border border-zinc-800 bg-zinc-900 overflow-hidden">
                            <div className="px-4 py-3 flex items-start gap-3">
                                <div className="flex-1 min-w-0">
                                    <div className="flex items-center gap-2">
                                        <span className="text-sm font-medium text-zinc-200">{event.name}</span>
                                        <Badge variant={event.enabled ? 'success' : 'outline'}>
                                            {event.enabled ? 'Enabled' : 'Disabled'}
                                        </Badge>
                                    </div>
                                    <code className="text-xs font-mono text-zinc-500 mt-0.5 block">
                                        {event.cron}
                                    </code>
                                    {event.next_run && (
                                        <p className="text-xs text-zinc-600 mt-1">
                                            Next: {formatDistanceToNow(new Date(event.next_run), { addSuffix: true })}
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
                                        className="p-1.5 rounded hover:bg-zinc-800 text-zinc-600 hover:text-red-400 transition-colors"
                                    >
                                        <Trash2 size={14} />
                                    </button>
                                </div>
                            </div>
                            {event.prompt && (
                                <div className="px-4 py-2 border-t border-zinc-800">
                                    <p className="text-xs text-zinc-500 truncate">{event.prompt}</p>
                                </div>
                            )}
                        </div>
                    ))}
                </div>
            )}
        </div>
    );
}
