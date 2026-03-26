import { Shield, Trash2, Copy, Check } from 'lucide-react';
import * as React from 'react';
import { Badge } from '../ui/badge';
import { Switch } from '../ui/switch';
import { trpc } from '../../api/trpc';
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

    return (
        <div className="rounded-xl border border-zinc-800 bg-zinc-900 overflow-hidden">
            <div className="px-4 py-3 flex items-center gap-3">
                {server.builtin && (
                    <Shield size={14} className="text-zinc-500 shrink-0" />
                )}
                <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2">
                        <span className="text-sm font-medium text-zinc-200">{server.name}</span>
                        <Badge variant={server.builtin ? 'outline' : 'default'}>
                            {server.builtin ? 'Built-in' : 'Custom'}
                        </Badge>
                        {server.url ? (
                            <Badge variant="outline">URL</Badge>
                        ) : (
                            <Badge variant="outline">Command</Badge>
                        )}
                    </div>
                    {commandStr && (
                        <div className="flex items-center gap-1 mt-1">
                            <code className="text-[11px] font-mono text-zinc-500 truncate max-w-xs">
                                {commandStr}
                            </code>
                            <button
                                onClick={handleCopy}
                                className="shrink-0 text-zinc-600 hover:text-zinc-400 transition-colors"
                                title="Copy command"
                            >
                                {copied ? <Check size={11} /> : <Copy size={11} />}
                            </button>
                        </div>
                    )}
                </div>

                <div className="flex items-center gap-2 shrink-0">
                    <Switch
                        checked={server.enabled}
                        onCheckedChange={enabled => toggleMutation.mutate({ name: server.name, enabled })}
                        disabled={toggleMutation.isPending}
                    />
                    {!server.builtin && (
                        <button
                            onClick={() => {
                                if (confirm(`Remove server "${server.name}"?`)) {
                                    removeMutation.mutate(server.name);
                                }
                            }}
                            className="p-1.5 rounded hover:bg-zinc-800 text-zinc-600 hover:text-red-400 transition-colors"
                            title="Remove"
                        >
                            <Trash2 size={14} />
                        </button>
                    )}
                </div>
            </div>

            {server.env && Object.keys(server.env).length > 0 && (
                <div className="px-4 py-2 border-t border-zinc-800 flex flex-wrap gap-1">
                    {Object.keys(server.env).map(key => (
                        <span key={key} className="text-[10px] font-mono text-zinc-500 bg-zinc-800 px-1.5 py-0.5 rounded">
                            {key}
                        </span>
                    ))}
                </div>
            )}
        </div>
    );
}
