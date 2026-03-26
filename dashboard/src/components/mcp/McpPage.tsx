import * as React from 'react';
import { Plus, Plug } from 'lucide-react';
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

    return (
        <div className="p-6 space-y-6 max-w-3xl">
            <div className="flex items-center gap-3">
                <div className="flex-1">
                    <h1 className="text-lg font-semibold text-zinc-100">MCP Servers</h1>
                    <p className="text-sm text-zinc-500">{servers.length} configured</p>
                </div>
                <Button onClick={() => setAddOpen(true)} size="sm">
                    <Plus size={13} />
                    Add Server
                </Button>
            </div>

            {serversQuery.isLoading ? (
                <div className="space-y-3">
                    {[...Array(3)].map((_, i) => <Skeleton key={i} className="h-16 rounded-xl" />)}
                </div>
            ) : (
                <>
                    {/* Built-in servers */}
                    {builtins.length > 0 && (
                        <div className="space-y-3">
                            <div className="flex items-center gap-2">
                                <Plug size={13} className="text-zinc-500" />
                                <h2 className="text-xs font-semibold text-zinc-500 uppercase tracking-wider">Built-in</h2>
                            </div>
                            {builtins.map(server => (
                                <McpServerCard key={server.name} server={server} />
                            ))}
                        </div>
                    )}

                    {/* Custom servers */}
                    {custom.length > 0 && (
                        <div className="space-y-3">
                            <div className="flex items-center gap-2">
                                <Plug size={13} className="text-zinc-500" />
                                <h2 className="text-xs font-semibold text-zinc-500 uppercase tracking-wider">Custom</h2>
                            </div>
                            {custom.map(server => (
                                <McpServerCard key={server.name} server={server} />
                            ))}
                        </div>
                    )}

                    {servers.length === 0 && (
                        <div className="flex flex-col items-center justify-center py-16 text-zinc-600">
                            <Plug size={24} className="mb-3" />
                            <p className="text-sm">No MCP servers configured</p>
                            <p className="text-xs mt-1">Add servers for Claude to use as tools</p>
                        </div>
                    )}
                </>
            )}

            <AddMcpDialog open={addOpen} onOpenChange={setAddOpen} />
        </div>
    );
}
