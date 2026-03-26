import { ArrowUp, ArrowDown, DollarSign, MessageSquare } from 'lucide-react';
import type { TaskInfo } from '../../types';
import { formatCost, formatTokens } from '../../lib/utils';

interface StatsPanelProps {
    task: TaskInfo;
}

function Stat({ icon, label, value }: { icon: React.ReactNode; label: string; value: string }) {
    return (
        <div className="flex items-center gap-1.5 text-xs shrink-0">
            <span className="text-zinc-700">{icon}</span>
            <span className="text-zinc-600">{label}</span>
            <span className="text-zinc-400 font-mono tabular-nums">{value}</span>
        </div>
    );
}

export function StatsPanel({ task }: StatsPanelProps) {
    return (
        <div className="flex items-center gap-4 px-4 py-2 border-b border-zinc-800/60 bg-zinc-900/30 overflow-x-auto shrink-0 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
            <div className="flex items-center gap-1 shrink-0 text-xs">
                <span className="text-zinc-700"><ArrowUp size={11} /></span>
                <span className="text-zinc-600">In</span>
                <span className="text-zinc-400 font-mono tabular-nums">{formatTokens(task.input_tokens)}</span>
                <span className="text-zinc-700 mx-1">/</span>
                <span className="text-zinc-700"><ArrowDown size={11} /></span>
                <span className="text-zinc-600">Out</span>
                <span className="text-zinc-400 font-mono tabular-nums">{formatTokens(task.output_tokens)}</span>
            </div>
            <Stat icon={<DollarSign size={11} />} label="Cost" value={formatCost(task.cost_usd)} />
            <Stat icon={<MessageSquare size={11} />} label="Turns" value={String(task.turns)} />
        </div>
    );
}
