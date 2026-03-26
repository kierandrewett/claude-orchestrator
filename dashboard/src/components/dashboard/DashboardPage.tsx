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
} from 'lucide-react';
import { AreaChart, Area, XAxis, YAxis, ResponsiveContainer, Tooltip } from 'recharts';
import { formatRelative } from 'date-fns';
import { trpc } from '../../api/trpc';
import { formatCost, formatTokens } from '../../lib/utils';
import { Skeleton } from '../ui/skeleton';

function MetricCard({
    label,
    value,
    subtitle,
    icon: Icon,
    color = 'emerald',
}: {
    label: string;
    value: string;
    subtitle?: string;
    icon: React.ComponentType<{ className?: string }>;
    color?: string;
}) {
    const colorMap: Record<string, { bg: string; text: string }> = {
        emerald: { bg: 'bg-emerald-500/10', text: 'text-emerald-400' },
        blue: { bg: 'bg-blue-500/10', text: 'text-blue-400' },
        violet: { bg: 'bg-violet-500/10', text: 'text-violet-400' },
        amber: { bg: 'bg-amber-500/10', text: 'text-amber-400' },
    };
    const colors = colorMap[color] ?? colorMap.emerald;

    return (
        <div className="rounded-xl border border-zinc-800 bg-zinc-900 p-4">
            <div className="flex items-center gap-2 mb-3">
                <div className={`w-8 h-8 rounded-lg ${colors.bg} flex items-center justify-center`}>
                    <Icon className={`w-4 h-4 ${colors.text}`} />
                </div>
                <span className="text-sm text-zinc-400">{label}</span>
            </div>
            <p className="text-2xl font-bold text-zinc-100 tabular-nums">{value}</p>
            {subtitle && <p className="text-xs text-zinc-500 mt-1">{subtitle}</p>}
        </div>
    );
}

function EventTypeBadge({ event }: { event: unknown }) {
    const e = event as Record<string, unknown>;
    const type = Object.keys(e)[0] ?? 'Unknown';
    const colorMap: Record<string, string> = {
        TaskCreated: 'text-emerald-400',
        TaskStateChanged: 'text-amber-400',
        TurnComplete: 'text-blue-400',
        ConversationRenamed: 'text-violet-400',
        Error: 'text-red-400',
        TextOutput: 'text-zinc-400',
        ToolStarted: 'text-zinc-500',
        ToolCompleted: 'text-zinc-500',
    };
    return (
        <span className={`text-[11px] font-medium ${colorMap[type] ?? 'text-zinc-500'}`}>
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
    const recentEvents = [...events].reverse().slice(0, 10);

    // Build cost chart data from task list
    const costData = React.useMemo(() => {
        const tasks = tasksQuery.data ?? [];
        const sorted = [...tasks].sort(
            (a, b) => new Date(a.created_at).getTime() - new Date(b.created_at).getTime()
        );
        let cumulative = 0;
        return sorted.map(t => {
            cumulative += t.cost_usd;
            return {
                name: t.name.slice(0, 12),
                cost: parseFloat(cumulative.toFixed(4)),
            };
        });
    }, [tasksQuery.data]);

    if (metricsQuery.isLoading) {
        return (
            <div className="p-6 space-y-6">
                <div className="grid grid-cols-2 lg:grid-cols-4 gap-4">
                    {[...Array(4)].map((_, i) => (
                        <Skeleton key={i} className="h-28 rounded-xl" />
                    ))}
                </div>
            </div>
        );
    }

    const runningCount = metrics?.running_tasks ?? 0;
    const hibCount = metrics?.hibernated_tasks ?? 0;
    const deadCount = metrics?.dead_tasks ?? 0;

    return (
        <div className="p-6 space-y-6 max-w-5xl">
            {/* Metric cards */}
            <div className="grid grid-cols-2 lg:grid-cols-4 gap-4">
                <MetricCard
                    label="Total Tasks"
                    value={String(metrics?.total_tasks ?? 0)}
                    subtitle={`${runningCount} running · ${hibCount} hibernated · ${deadCount} dead`}
                    icon={Activity}
                    color="emerald"
                />
                <MetricCard
                    label="Total Cost"
                    value={formatCost(metrics?.total_cost_usd ?? 0)}
                    subtitle="across all tasks"
                    icon={DollarSign}
                    color="blue"
                />
                <MetricCard
                    label="Total Tokens"
                    value={formatTokens((metrics?.total_input_tokens ?? 0) + (metrics?.total_output_tokens ?? 0))}
                    subtitle={`${formatTokens(metrics?.total_input_tokens ?? 0)} in · ${formatTokens(metrics?.total_output_tokens ?? 0)} out`}
                    icon={Hash}
                    color="violet"
                />
                <MetricCard
                    label="Active Sessions"
                    value={String(runningCount)}
                    subtitle={runningCount > 0 ? 'tasks are running' : 'no active tasks'}
                    icon={Zap}
                    color="amber"
                />
            </div>

            <div className="grid lg:grid-cols-2 gap-6">
                {/* Cost chart */}
                {costData.length > 1 && (
                    <div className="rounded-xl border border-zinc-800 bg-zinc-900 overflow-hidden">
                        <div className="px-4 py-3 border-b border-zinc-800">
                            <h3 className="text-sm font-semibold text-zinc-200">Cumulative Cost</h3>
                        </div>
                        <div className="p-4 h-48">
                            <ResponsiveContainer width="100%" height="100%">
                                <AreaChart data={costData}>
                                    <defs>
                                        <linearGradient id="costGrad" x1="0" y1="0" x2="0" y2="1">
                                            <stop offset="5%" stopColor="#10b981" stopOpacity={0.3} />
                                            <stop offset="95%" stopColor="#10b981" stopOpacity={0} />
                                        </linearGradient>
                                    </defs>
                                    <XAxis dataKey="name" tick={{ fontSize: 10, fill: '#71717a' }} tickLine={false} axisLine={false} />
                                    <YAxis tick={{ fontSize: 10, fill: '#71717a' }} tickLine={false} axisLine={false} tickFormatter={v => `$${(v as number).toFixed(3)}`} />
                                    <Tooltip
                                        contentStyle={{ background: '#18181b', border: '1px solid #3f3f46', borderRadius: 8, fontSize: 12 }}
                                        formatter={(v) => [`$${(v as number).toFixed(4)}`, 'Cost']}
                                    />
                                    <Area type="monotone" dataKey="cost" stroke="#10b981" strokeWidth={2} fill="url(#costGrad)" />
                                </AreaChart>
                            </ResponsiveContainer>
                        </div>
                    </div>
                )}

                {/* Recent activity */}
                <div className="rounded-xl border border-zinc-800 bg-zinc-900 overflow-hidden">
                    <div className="px-4 py-3 border-b border-zinc-800">
                        <h3 className="text-sm font-semibold text-zinc-200">Recent Activity</h3>
                    </div>
                    <div className="divide-y divide-zinc-800/60">
                        {recentEvents.length === 0 ? (
                            <div className="px-4 py-8 text-center text-sm text-zinc-600">
                                No recent events
                            </div>
                        ) : (
                            recentEvents.map((entry, i) => (
                                <div key={entry.id ?? i} className="px-4 py-2.5 flex items-center gap-3">
                                    <EventTypeBadge event={entry.event} />
                                    <span className="text-xs text-zinc-500 ml-auto shrink-0">
                                        {formatRelative(new Date(entry.timestamp), new Date())}
                                    </span>
                                </div>
                            ))
                        )}
                    </div>
                </div>
            </div>

            {/* Quick actions */}
            <div className="rounded-xl border border-zinc-800 bg-zinc-900 overflow-hidden">
                <div className="px-4 py-3 border-b border-zinc-800">
                    <h3 className="text-sm font-semibold text-zinc-200">Quick Actions</h3>
                </div>
                <div className="p-4 flex flex-wrap gap-2">
                    <Link
                        to="/tasks"
                        className="inline-flex items-center gap-2 px-3 py-2 rounded-lg bg-zinc-800 hover:bg-zinc-700 text-sm text-zinc-200 transition-colors border border-zinc-700"
                    >
                        <Plus size={14} />
                        New Task
                    </Link>
                    <Link
                        to="/config"
                        className="inline-flex items-center gap-2 px-3 py-2 rounded-lg bg-zinc-800 hover:bg-zinc-700 text-sm text-zinc-200 transition-colors border border-zinc-700"
                    >
                        <Settings2 size={14} />
                        View Config
                    </Link>
                    <Link
                        to="/mcp"
                        className="inline-flex items-center gap-2 px-3 py-2 rounded-lg bg-zinc-800 hover:bg-zinc-700 text-sm text-zinc-200 transition-colors border border-zinc-700"
                    >
                        <Plug size={14} />
                        Manage MCP
                    </Link>
                </div>
            </div>
        </div>
    );
}
