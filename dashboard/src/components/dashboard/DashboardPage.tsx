import * as React from 'react';
import { Link } from '@tanstack/react-router';
import {
    Activity,
    DollarSign,
    Hash,
    Zap,
    Plus,
    Settings2,
    Plug,
    ArrowUpRight,
    Clock,
} from 'lucide-react';
import { AreaChart, Area, XAxis, YAxis, ResponsiveContainer, Tooltip } from 'recharts';
import { formatDistanceToNow, formatRelative } from 'date-fns';
import { trpc } from '../../api/trpc';
import { formatCost, formatTokens, getStatusDot } from '../../lib/utils';
import { Skeleton } from '../ui/skeleton';
import { cn } from '../../lib/utils';

function MetricCard({
    label,
    value,
    subtitle,
    icon: Icon,
    accent = 'emerald',
    href,
}: {
    label: string;
    value: string;
    subtitle?: string;
    icon: React.ComponentType<{ size?: number; className?: string }>;
    accent?: 'emerald' | 'blue' | 'violet' | 'amber';
    href?: string;
}) {
    const accentMap: Record<string, { icon: string; value: string }> = {
        emerald: { icon: 'text-emerald-400', value: 'text-zinc-100' },
        blue:    { icon: 'text-blue-400',    value: 'text-zinc-100' },
        violet:  { icon: 'text-violet-400',  value: 'text-zinc-100' },
        amber:   { icon: 'text-amber-400',   value: 'text-zinc-100' },
    };
    const colors = accentMap[accent] ?? accentMap.emerald;

    const inner = (
        <div className={cn(
            'group rounded-xl border border-zinc-800/80 bg-zinc-900/50 p-4 flex flex-col gap-3 transition-colors',
            href && 'hover:border-zinc-700 hover:bg-zinc-900 cursor-pointer',
        )}>
            <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                    <Icon size={14} className={colors.icon} />
                    <span className="text-xs font-medium text-zinc-500">{label}</span>
                </div>
                {href && (
                    <ArrowUpRight size={12} className="text-zinc-700 group-hover:text-zinc-500 transition-colors" />
                )}
            </div>
            <div>
                <p className={cn('text-2xl font-bold tabular-nums leading-none', colors.value)}>{value}</p>
                {subtitle && <p className="text-xs text-zinc-600 mt-1.5">{subtitle}</p>}
            </div>
        </div>
    );

    return href ? <Link to={href}>{inner}</Link> : inner;
}

const EVENT_TYPE_COLORS: Record<string, string> = {
    TaskCreated:      'text-emerald-400 bg-emerald-500/8 border-emerald-500/15',
    TaskStateChanged: 'text-amber-400 bg-amber-500/8 border-amber-500/15',
    TurnComplete:     'text-blue-400 bg-blue-500/8 border-blue-500/15',
    ConversationRenamed: 'text-violet-400 bg-violet-500/8 border-violet-500/15',
    Error:            'text-red-400 bg-red-500/8 border-red-500/15',
    TextOutput:       'text-zinc-400 bg-zinc-800/40 border-zinc-700/30',
    ToolStarted:      'text-zinc-500 bg-zinc-800/30 border-zinc-700/20',
    ToolCompleted:    'text-zinc-500 bg-zinc-800/30 border-zinc-700/20',
};

function EventTypeBadge({ event }: { event: unknown }) {
    const e = event as Record<string, unknown>;
    const type = Object.keys(e)[0] ?? 'Unknown';
    const cls = EVENT_TYPE_COLORS[type] ?? 'text-zinc-500 bg-zinc-800/30 border-zinc-700/20';
    return (
        <span className={cn('text-[10px] font-semibold px-1.5 py-0.5 rounded border', cls)}>
            {type}
        </span>
    );
}

export function DashboardPage() {
    const metricsQuery = trpc.metrics.summary.useQuery(undefined, { refetchInterval: 5000 });
    const eventLogQuery = trpc.metrics.eventLog.useQuery(undefined, { refetchInterval: 3000 });
    const tasksQuery = trpc.tasks.list.useQuery(undefined, { refetchInterval: 5000 });

    const metrics = metricsQuery.data;
    const events = eventLogQuery.data ?? [];
    const recentEvents = [...events].reverse().slice(0, 8);
    const tasks = tasksQuery.data ?? [];

    const costData = React.useMemo(() => {
        const sorted = [...tasks].sort(
            (a, b) => new Date(a.created_at).getTime() - new Date(b.created_at).getTime()
        );
        let cumulative = 0;
        return sorted.map(t => {
            cumulative += t.cost_usd;
            return { name: t.name.slice(0, 10), cost: parseFloat(cumulative.toFixed(4)) };
        });
    }, [tasks]);

    const recentTasks = [...tasks]
        .sort((a, b) => new Date(b.last_activity ?? b.created_at).getTime() - new Date(a.last_activity ?? a.created_at).getTime())
        .slice(0, 5);

    if (metricsQuery.isLoading) {
        return (
            <div className="p-6 space-y-6">
                <div className="grid grid-cols-2 lg:grid-cols-4 gap-3">
                    {[...Array(4)].map((_, i) => <Skeleton key={i} className="h-28 rounded-xl" />)}
                </div>
            </div>
        );
    }

    const runningCount = metrics?.running_tasks ?? 0;
    const hibCount = metrics?.hibernated_tasks ?? 0;
    const deadCount = metrics?.dead_tasks ?? 0;
    const totalTasks = metrics?.total_tasks ?? 0;

    return (
        <div className="p-4 md:p-6 space-y-5 max-w-5xl">
            {/* Metric cards */}
            <div className="grid grid-cols-2 lg:grid-cols-4 gap-3">
                <MetricCard
                    label="Total Tasks"
                    value={String(totalTasks)}
                    subtitle={`${runningCount} running · ${hibCount} hibernated`}
                    icon={Activity}
                    accent="emerald"
                    href="/tasks"
                />
                <MetricCard
                    label="Total Cost"
                    value={formatCost(metrics?.total_cost_usd ?? 0)}
                    subtitle="across all tasks"
                    icon={DollarSign}
                    accent="blue"
                />
                <MetricCard
                    label="Tokens Used"
                    value={formatTokens((metrics?.total_input_tokens ?? 0) + (metrics?.total_output_tokens ?? 0))}
                    subtitle={`${formatTokens(metrics?.total_input_tokens ?? 0)} in · ${formatTokens(metrics?.total_output_tokens ?? 0)} out`}
                    icon={Hash}
                    accent="violet"
                />
                <MetricCard
                    label="Active Sessions"
                    value={String(runningCount)}
                    subtitle={runningCount > 0 ? `${deadCount} finished` : 'no active tasks'}
                    icon={Zap}
                    accent="amber"
                    href={runningCount > 0 ? '/tasks' : undefined}
                />
            </div>

            <div className="grid lg:grid-cols-5 gap-4">
                {/* Left column: recent tasks + cost chart */}
                <div className="lg:col-span-3 space-y-4">
                    {/* Recent tasks */}
                    {recentTasks.length > 0 && (
                        <div className="rounded-xl border border-zinc-800/80 bg-zinc-900/50 overflow-hidden">
                            <div className="flex items-center justify-between px-4 py-3 border-b border-zinc-800/60">
                                <h3 className="text-xs font-semibold text-zinc-400 uppercase tracking-wider">Recent Tasks</h3>
                                <Link to="/tasks" className="text-[11px] text-zinc-500 hover:text-zinc-300 transition-colors flex items-center gap-1">
                                    View all <ArrowUpRight size={10} />
                                </Link>
                            </div>
                            <div className="divide-y divide-zinc-800/40">
                                {recentTasks.map(task => (
                                    <Link
                                        key={task.id}
                                        to="/tasks/$id"
                                        params={{ id: task.id }}
                                        className="flex items-center gap-3 px-4 py-2.5 hover:bg-zinc-800/30 transition-colors"
                                    >
                                        <span className={cn(
                                            'w-1.5 h-1.5 rounded-full shrink-0',
                                            getStatusDot(task.state),
                                            task.state === 'Running' && 'animate-pulse-dot',
                                        )} />
                                        <span className="text-sm text-zinc-200 flex-1 truncate">{task.name}</span>
                                        <span className="text-xs text-zinc-600 font-mono shrink-0">{formatCost(task.cost_usd)}</span>
                                        <span className="text-xs text-zinc-600 shrink-0">
                                            {task.last_activity
                                                ? formatDistanceToNow(new Date(task.last_activity), { addSuffix: true })
                                                : '—'}
                                        </span>
                                    </Link>
                                ))}
                            </div>
                        </div>
                    )}

                    {/* Cost chart */}
                    {costData.length > 1 && (
                        <div className="rounded-xl border border-zinc-800/80 bg-zinc-900/50 overflow-hidden">
                            <div className="px-4 py-3 border-b border-zinc-800/60">
                                <h3 className="text-xs font-semibold text-zinc-400 uppercase tracking-wider">Cumulative Cost</h3>
                            </div>
                            <div className="p-4 h-44">
                                <ResponsiveContainer width="100%" height="100%">
                                    <AreaChart data={costData} margin={{ top: 4, right: 4, bottom: 0, left: -20 }}>
                                        <defs>
                                            <linearGradient id="costGrad" x1="0" y1="0" x2="0" y2="1">
                                                <stop offset="5%" stopColor="#10b981" stopOpacity={0.2} />
                                                <stop offset="95%" stopColor="#10b981" stopOpacity={0} />
                                            </linearGradient>
                                        </defs>
                                        <XAxis
                                            dataKey="name"
                                            tick={{ fontSize: 10, fill: '#52525b' }}
                                            tickLine={false}
                                            axisLine={false}
                                        />
                                        <YAxis
                                            tick={{ fontSize: 10, fill: '#52525b' }}
                                            tickLine={false}
                                            axisLine={false}
                                            tickFormatter={v => `$${(v as number).toFixed(2)}`}
                                        />
                                        <Tooltip
                                            contentStyle={{
                                                background: '#09090b',
                                                border: '1px solid #27272a',
                                                borderRadius: 8,
                                                fontSize: 11,
                                                color: '#e4e4e7',
                                            }}
                                            formatter={(v) => [`$${(v as number).toFixed(4)}`, 'Cost']}
                                        />
                                        <Area
                                            type="monotone"
                                            dataKey="cost"
                                            stroke="#10b981"
                                            strokeWidth={1.5}
                                            fill="url(#costGrad)"
                                        />
                                    </AreaChart>
                                </ResponsiveContainer>
                            </div>
                        </div>
                    )}
                </div>

                {/* Right column: activity + quick actions */}
                <div className="lg:col-span-2 space-y-4">
                    {/* Recent activity */}
                    <div className="rounded-xl border border-zinc-800/80 bg-zinc-900/50 overflow-hidden">
                        <div className="px-4 py-3 border-b border-zinc-800/60">
                            <h3 className="text-xs font-semibold text-zinc-400 uppercase tracking-wider">Activity</h3>
                        </div>
                        <div className="divide-y divide-zinc-800/40 max-h-64 overflow-y-auto">
                            {recentEvents.length === 0 ? (
                                <div className="px-4 py-8 text-center text-xs text-zinc-600">
                                    <Clock size={20} className="mx-auto mb-2 opacity-40" />
                                    No recent events
                                </div>
                            ) : (
                                recentEvents.map((entry, i) => (
                                    <div key={entry.id ?? i} className="px-4 py-2 flex items-center gap-2">
                                        <EventTypeBadge event={entry.event} />
                                        <span className="text-[10px] text-zinc-600 ml-auto shrink-0 whitespace-nowrap">
                                            {formatRelative(new Date(entry.timestamp), new Date())}
                                        </span>
                                    </div>
                                ))
                            )}
                        </div>
                    </div>

                    {/* Quick actions */}
                    <div className="rounded-xl border border-zinc-800/80 bg-zinc-900/50 overflow-hidden">
                        <div className="px-4 py-3 border-b border-zinc-800/60">
                            <h3 className="text-xs font-semibold text-zinc-400 uppercase tracking-wider">Quick Actions</h3>
                        </div>
                        <div className="p-3 flex flex-col gap-1.5">
                            <Link
                                to="/tasks"
                                className="flex items-center gap-2.5 px-3 py-2 rounded-lg text-sm text-zinc-300 hover:bg-zinc-800/60 hover:text-zinc-100 transition-colors"
                            >
                                <Plus size={14} className="text-zinc-500" />
                                New Task
                            </Link>
                            <Link
                                to="/mcp"
                                className="flex items-center gap-2.5 px-3 py-2 rounded-lg text-sm text-zinc-300 hover:bg-zinc-800/60 hover:text-zinc-100 transition-colors"
                            >
                                <Plug size={14} className="text-zinc-500" />
                                MCP Servers
                            </Link>
                            <Link
                                to="/config"
                                className="flex items-center gap-2.5 px-3 py-2 rounded-lg text-sm text-zinc-300 hover:bg-zinc-800/60 hover:text-zinc-100 transition-colors"
                            >
                                <Settings2 size={14} className="text-zinc-500" />
                                Configuration
                            </Link>
                        </div>
                    </div>
                </div>
            </div>
        </div>
    );
}
