import * as React from 'react';
import { Plus, Plug, Info, RefreshCw } from 'lucide-react';
import { trpc } from '../../api/trpc';
import { McpServerCard } from './McpServerCard';
import { AddMcpDialog } from './AddMcpDialog';
import { Button } from '../ui/button';
import { Skeleton } from '../ui/skeleton';

export function McpPage() {
    const [addOpen, setAddOpen] = React.useState(false);
    const serversQuery = trpc.mcp.list.useQuery(undefined, { refetchInterval: 10_000 });
    const servers = serversQuery.data ?? [];

    const builtins = servers.filter(s => s.builtin);
    const custom = servers.filter(s => !s.builtin);
    const enabledCount = servers.filter(s => s.enabled).length;

    return (
        <div className="p-4 md:p-6 space-y-5 max-w-2xl">
            {/* Header */}
            <div className="flex items-center gap-3">
                <div className="flex-1">
                    <h1 className="text-sm font-semibold text-zinc-100">MCP Servers</h1>
                    <p className="text-xs text-zinc-600 mt-0.5">
                        {enabledCount} of {servers.length} enabled
                    </p>
                </div>
                <Button onClick={() => setAddOpen(true)} size="sm">
                    <Plus size={13} />
                    Add Server
                </Button>
            </div>

            {/* Informational banner */}
            <div className="flex gap-3 p-3.5 rounded-xl border border-blue-500/20 bg-blue-500/5">
                <Info size={14} className="text-blue-400 shrink-0 mt-0.5" />
                <div className="space-y-1">
                    <p className="text-xs font-medium text-blue-300">How MCP servers work</p>
                    <p className="text-xs text-zinc-500 leading-relaxed">
                        MCP servers are loaded at session start. When you add, remove, or toggle a server,
                        any hibernated tasks are automatically restarted so they pick up the new config.
                    </p>
                    <p className="text-xs text-zinc-600 flex items-center gap-1 mt-1">
                        <RefreshCw size={10} />
                        Changes are saved to{' '}
                        <code className="font-mono text-zinc-500 bg-zinc-800/60 px-1 py-px rounded text-[10px]">
                            mcp_servers.json
                        </code>
                    </p>
                </div>
            </div>

            {serversQuery.isLoading ? (
                <div className="space-y-2">
                    {[...Array(3)].map((_, i) => <Skeleton key={i} className="h-16 rounded-xl" />)}
                </div>
            ) : (
                <>
                    {/* Built-in servers */}
                    {builtins.length > 0 && (
                        <div className="space-y-2">
                            <p className="text-[10px] font-semibold text-zinc-600 uppercase tracking-wider px-1">
                                Built-in
                            </p>
                            {builtins.map(server => (
                                <McpServerCard key={server.name} server={server} />
                            ))}
                        </div>
                    )}

                    {/* Custom servers */}
                    {custom.length > 0 && (
                        <div className="space-y-2">
                            <p className="text-[10px] font-semibold text-zinc-600 uppercase tracking-wider px-1">
                                Custom
                            </p>
                            {custom.map(server => (
                                <McpServerCard key={server.name} server={server} />
                            ))}
                        </div>
                    )}

                    {servers.length === 0 && (
                        <div className="flex flex-col items-center justify-center py-16 text-center">
                            <div className="w-12 h-12 rounded-xl bg-zinc-900 border border-zinc-800 flex items-center justify-center mb-4">
                                <Plug size={20} className="text-zinc-700" />
                            </div>
                            <p className="text-sm font-medium text-zinc-500">No MCP servers configured</p>
                            <p className="text-xs text-zinc-700 mt-1 max-w-xs">
                                Add MCP servers to give Claude access to extra tools like filesystems, APIs, and databases.
                            </p>
                            <button
                                onClick={() => setAddOpen(true)}
                                className="mt-4 flex items-center gap-1.5 text-xs text-zinc-400 hover:text-zinc-200 border border-zinc-800 hover:border-zinc-700 px-3 py-1.5 rounded-lg transition-colors"
                            >
                                <Plus size={12} /> Add your first server
                            </button>
                        </div>
                    )}
                </>
            )}

            <AddMcpDialog open={addOpen} onOpenChange={setAddOpen} />
        </div>
    );
}
