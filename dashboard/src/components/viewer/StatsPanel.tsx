import { Coins, Hash } from 'lucide-react';
import type { TaskInfo } from '../../types';
import { formatCost, formatTokens } from '../../lib/utils';

interface StatsPanelProps {
    task: TaskInfo;
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

export function StatsPanel({ task }: StatsPanelProps) {
    return (
        <div className="flex items-center gap-4 px-4 py-2 border-b border-zinc-800 bg-zinc-900/50 overflow-x-auto shrink-0 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
            <div className="flex items-center gap-1.5 text-xs text-zinc-400 shrink-0">
                <span className="text-zinc-600"><Coins size={12} /></span>
                <span className="text-zinc-600">Tokens</span>
                <span className="text-zinc-300 font-mono whitespace-nowrap">
                    {formatTokens(task.input_tokens)}
                    <span className="text-zinc-600">↑</span>
                    {formatTokens(task.output_tokens)}
                    <span className="text-zinc-600">↓</span>
                </span>
            </div>
            <StatItem icon={<span>$</span>} label="Cost" value={formatCost(task.cost_usd)} />
            <StatItem icon={<Hash size={12} />} label="Turns" value={String(task.turns)} />
        </div>
    );
}
