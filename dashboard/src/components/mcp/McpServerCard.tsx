import { Shield, Trash2, Copy, Check, Terminal, Globe, AlertCircle } from 'lucide-react';
import * as React from 'react';
import { Switch } from '../ui/switch';
import { trpc } from '../../api/trpc';
import { cn } from '../../lib/utils';
import type { McpServer } from '../../server-types';

interface McpServerCardProps {
    server: McpServer;
}

export function McpServerCard({ server }: McpServerCardProps) {
    const [copied, setCopied] = React.useState(false);
    const utils = trpc.useUtils();

    const toggleMutation = trpc.mcp.toggle.useMutation({
        onSuccess: () => utils.mcp.list.invalidate(),
    });
    const removeMutation = trpc.mcp.remove.useMutation({
        onSuccess: () => utils.mcp.list.invalidate(),
    });

    const commandStr = server.command
        ? [server.command, ...(server.args ?? [])].join(' ')
        : server.url ?? '';

    const handleCopy = async () => {
        await navigator.clipboard.writeText(commandStr);
        setCopied(true);
        setTimeout(() => setCopied(false), 2000);
    };

    const isUrl = !!server.url;

    return (
        <div className={cn(
            'rounded-xl border bg-zinc-900/50 overflow-hidden transition-colors',
            server.enabled ? 'border-zinc-800/80' : 'border-zinc-800/40 opacity-60',
        )}>
            <div className="px-4 py-3 flex items-center gap-3">
                {/* Icon */}
                <div className="w-7 h-7 rounded-md bg-zinc-800 flex items-center justify-center shrink-0">
                    {server.builtin ? (
                        <Shield size={13} className="text-zinc-400" />
                    ) : isUrl ? (
                        <Globe size={13} className="text-zinc-400" />
                    ) : (
                        <Terminal size={13} className="text-zinc-400" />
                    )}
                </div>

                {/* Info */}
                <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2 flex-wrap">
                        <span className="text-sm font-medium text-zinc-200">{server.name}</span>
                        {server.builtin && (
                            <span className="text-[9px] font-semibold px-1.5 py-0.5 rounded-full border border-zinc-700/50 text-zinc-500 bg-zinc-800/60 uppercase tracking-wide">
                                Built-in
                            </span>
                        )}
                        <span className="text-[9px] font-semibold px-1.5 py-0.5 rounded-full border border-zinc-700/50 text-zinc-600 bg-zinc-800/40 uppercase tracking-wide">
                            {isUrl ? 'URL' : 'Command'}
                        </span>
                        {server.connected === true && (
                            <span className="flex items-center gap-1 text-[9px] font-semibold px-1.5 py-0.5 rounded-full border border-emerald-500/20 text-emerald-400 bg-emerald-500/10 uppercase tracking-wide">
                                <span className="w-1.5 h-1.5 rounded-full bg-emerald-400 inline-block" />
                                Connected
                            </span>
                        )}
                        {server.connected === false && server.enabled && (
                            <span className="flex items-center gap-1 text-[9px] font-semibold px-1.5 py-0.5 rounded-full border border-red-500/20 text-red-400 bg-red-500/10 uppercase tracking-wide">
                                <AlertCircle size={9} />
                                Not connected
                            </span>
                        )}
                    </div>
                    {commandStr && (
                        <div className="flex items-center gap-1 mt-0.5">
                            <code className="text-[11px] font-mono text-zinc-600 truncate max-w-[240px] md:max-w-xs">
                                {commandStr}
                            </code>
                            <button
                                onClick={handleCopy}
                                className="shrink-0 text-zinc-700 hover:text-zinc-400 transition-colors p-0.5"
                                title="Copy"
                            >
                                {copied ? <Check size={10} className="text-emerald-400" /> : <Copy size={10} />}
                            </button>
                        </div>
                    )}
                </div>

                {/* Controls */}
                <div className="flex items-center gap-2 shrink-0">
                    <Switch
                        checked={server.enabled}
                        onCheckedChange={enabled => toggleMutation.mutate({ name: server.name, enabled })}
                        disabled={toggleMutation.isPending}
                    />
                    {!server.builtin && (
                        <button
                            onClick={() => {
                                if (confirm(`Remove "${server.name}"?`)) {
                                    removeMutation.mutate(server.name);
                                }
                            }}
                            className="p-1.5 rounded-md hover:bg-zinc-800 text-zinc-700 hover:text-red-400 transition-colors"
                            title="Remove server"
                        >
                            <Trash2 size={13} />
                        </button>
                    )}
                </div>
            </div>

            {/* Env vars */}
            {server.env && Object.keys(server.env).length > 0 && (
                <div className="px-4 py-2 border-t border-zinc-800/40 flex flex-wrap gap-1.5">
                    {Object.keys(server.env).map(key => (
                        <span
                            key={key}
                            className="text-[10px] font-mono text-zinc-600 bg-zinc-800/60 px-1.5 py-0.5 rounded border border-zinc-700/40"
                        >
                            {key}
                        </span>
                    ))}
                </div>
            )}
        </div>
    );
}
