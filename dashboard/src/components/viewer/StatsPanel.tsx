import { Clock, Coins, Hash, Wrench, AlertCircle } from 'lucide-react';
import type { SessionInfo } from '../../types';
import { formatCost, formatTokens, getStatusBgColor, cn } from '../../lib/utils';
import { useLiveDuration } from '../../hooks/useLiveDuration';

interface StatsPanelProps {
    session: SessionInfo;
}

function StatItem({ icon, label, value }: { icon: React.ReactNode; label: string; value: string }) {
    return (
        <div className="flex items-center gap-1.5 text-xs text-zinc-400 shrink-0">
            <span className="text-zinc-600">{icon}</span>
            <span className="text-zinc-600">{label}</span>
            <span className="text-zinc-300 font-mono">{value}</span>
        </div>
    );
}

export function StatsPanel({ session }: StatsPanelProps) {
    const duration = useLiveDuration(
        session.started_at,
        session.ended_at,
        session.status === 'running',
    );

    const { stats } = session;

    const topTools = Object.entries(stats.tool_calls ?? {})
        .sort(([, a], [, b]) => b - a)
        .slice(0, 3);
    const totalToolTypes = Object.keys(stats.tool_calls ?? {}).length;
    const remainingTools = totalToolTypes - topTools.length;

    return (
        <div className="flex items-center gap-4 px-4 py-2 border-b border-zinc-800 bg-zinc-900/50 overflow-x-auto shrink-0 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
            <StatItem icon={<Clock size={12} />} label="Duration" value={duration} />

            <div className="flex items-center gap-1.5 text-xs text-zinc-400 shrink-0">
                <span className="text-zinc-600"><Coins size={12} /></span>
                <span className="text-zinc-600">Tokens</span>
                <span className="text-zinc-300 font-mono whitespace-nowrap">
                    {formatTokens(stats.input_tokens)}
                    <span className="text-zinc-600">↑</span>
                    {formatTokens(stats.output_tokens)}
                    <span className="text-zinc-600">↓</span>
                </span>
            </div>

            <StatItem icon={<span>$</span>} label="Cost" value={formatCost(stats.cost_usd)} />
            <StatItem icon={<Hash size={12} />} label="Turns" value={String(stats.turns)} />

            {topTools.length > 0 && (
                <div className="flex items-center gap-1.5 text-xs text-zinc-400 shrink-0">
                    <span className="text-zinc-600"><Wrench size={12} /></span>
                    <span className="text-zinc-600">Tools</span>
                    <div className="flex items-center gap-1 font-mono">
                        {topTools.map(([name, count]) => (
                            <span key={name} className="text-zinc-300 whitespace-nowrap">
                                {name}<span className="text-zinc-500">×{count}</span>
                            </span>
                        ))}
                        {remainingTools > 0 && (
                            <span className="text-zinc-500">+{remainingTools}</span>
                        )}
                    </div>
                </div>
            )}

            {stats.stop_reason && (
                <div className="flex items-center gap-1.5 shrink-0">
                    <AlertCircle size={12} className="text-zinc-600" />
                    <span
                        className={cn(
                            'text-[10px] font-medium px-1.5 py-0.5 rounded ring-1 ring-inset whitespace-nowrap',
                            getStatusBgColor(
                                stats.stop_reason === 'end_turn'
                                    ? 'completed'
                                    : stats.stop_reason === 'max_tokens'
                                      ? 'failed'
                                      : 'pending',
                            ),
                        )}
                    >
                        {stats.stop_reason}
                    </span>
                </div>
            )}
        </div>
    );
}
