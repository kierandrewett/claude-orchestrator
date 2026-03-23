import { clsx, type ClassValue } from 'clsx';
import { twMerge } from 'tailwind-merge';
import type { TaskState } from '../types';

export function cn(...inputs: ClassValue[]) {
    return twMerge(clsx(inputs));
}

export function formatDuration(startedAt?: string, endedAt?: string): string {
    if (!startedAt) return '—';
    const start = new Date(startedAt).getTime();
    const end = endedAt ? new Date(endedAt).getTime() : Date.now();
    const totalSeconds = Math.floor((end - start) / 1000);
    if (totalSeconds < 0) return '—';
    const minutes = Math.floor(totalSeconds / 60);
    const seconds = totalSeconds % 60;
    if (minutes === 0) return `${seconds}s`;
    return `${minutes}m ${seconds}s`;
}

export function formatCost(costUsd?: number): string {
    if (costUsd === undefined || costUsd === null) return '—';
    if (costUsd === 0) return '$0.00';
    if (costUsd < 0.001) return `$${costUsd.toFixed(5)}`;
    if (costUsd < 0.01) return `$${costUsd.toFixed(4)}`;
    return `$${costUsd.toFixed(3)}`;
}

export function formatTokens(n: number): string {
    if (n === 0) return '0';
    if (n < 1000) return String(n);
    if (n < 1_000_000) return `${(n / 1000).toFixed(1)}k`;
    return `${(n / 1_000_000).toFixed(2)}M`;
}

export function getStatusColor(state: TaskState | string): string {
    switch (state) {
        case 'Running': return 'text-emerald-400';
        case 'Hibernated': return 'text-amber-400';
        case 'Dead': return 'text-zinc-400';
        default: return 'text-zinc-500';
    }
}

export function getStatusBgColor(state: TaskState | string): string {
    switch (state) {
        case 'Running': return 'bg-emerald-400/10 text-emerald-400 ring-emerald-400/20';
        case 'Hibernated': return 'bg-amber-400/10 text-amber-400 ring-amber-400/20';
        case 'Dead': return 'bg-zinc-400/10 text-zinc-400 ring-zinc-400/20';
        default: return 'bg-zinc-500/10 text-zinc-500 ring-zinc-500/20';
    }
}

export function getStatusDot(state: TaskState | string): string {
    switch (state) {
        case 'Running': return 'bg-emerald-400';
        case 'Hibernated': return 'bg-amber-400';
        case 'Dead': return 'bg-zinc-400';
        default: return 'bg-zinc-500';
    }
}
