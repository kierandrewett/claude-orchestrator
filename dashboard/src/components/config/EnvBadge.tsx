interface EnvBadgeProps {
    value: string;
}

export function EnvBadge({ value }: EnvBadgeProps) {
    const varName = value.replace(/^env:/, '');
    return (
        <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded text-xs bg-amber-500/10 text-amber-400 border border-amber-500/20 font-mono">
            <span className="text-amber-500">env:</span>
            {varName}
        </span>
    );
}

export function isEnvValue(value: unknown): value is string {
    return typeof value === 'string' && value.startsWith('env:');
}
