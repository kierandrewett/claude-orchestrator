import * as React from 'react';
import { cn } from '../../lib/utils';

export interface BadgeProps extends React.HTMLAttributes<HTMLSpanElement> {
    variant?: 'default' | 'success' | 'warning' | 'destructive' | 'outline';
}

const variantClasses: Record<NonNullable<BadgeProps['variant']>, string> = {
    default: 'bg-zinc-700 text-zinc-200 border-zinc-600',
    success: 'bg-emerald-500/15 text-emerald-400 border-emerald-500/30',
    warning: 'bg-amber-500/15 text-amber-400 border-amber-500/30',
    destructive: 'bg-red-500/15 text-red-400 border-red-500/30',
    outline: 'bg-transparent text-zinc-400 border-zinc-700',
};

export function Badge({ className, variant = 'default', ...props }: BadgeProps) {
    return (
        <span
            className={cn(
                'inline-flex items-center px-1.5 py-0.5 rounded text-[11px] font-medium border',
                variantClasses[variant],
                className,
            )}
            {...props}
        />
    );
}
