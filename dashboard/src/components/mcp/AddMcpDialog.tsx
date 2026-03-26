import * as React from 'react';
import {
    Dialog,
    DialogContent,
    DialogHeader,
    DialogTitle,
    DialogDescription,
    DialogFooter,
} from '../ui/dialog';
import { Button } from '../ui/button';
import { Input } from '../ui/input';
import { Label } from '../ui/label';
import { SegmentedControl, SegmentedControlItem } from '../ui/segmented-control';
import { trpc } from '../../api/trpc';
import { X, Plus } from 'lucide-react';

interface AddMcpDialogProps {
    open: boolean;
    onOpenChange: (open: boolean) => void;
}

type TransportType = 'command' | 'url';

export function AddMcpDialog({ open, onOpenChange }: AddMcpDialogProps) {
    const [name, setName] = React.useState('');
    const [transport, setTransport] = React.useState<TransportType>('command');
    const [command, setCommand] = React.useState('');
    const [argsInput, setArgsInput] = React.useState('');
    const [args, setArgs] = React.useState<string[]>([]);
    const [url, setUrl] = React.useState('');
    const [envPairs, setEnvPairs] = React.useState<Array<{ key: string; value: string }>>([]);

    const utils = trpc.useUtils();
    const addMutation = trpc.mcp.add.useMutation({
        onSuccess: () => {
            void utils.mcp.list.invalidate();
            onOpenChange(false);
            reset();
        },
    });

    const reset = () => {
        setName('');
        setTransport('command');
        setCommand('');
        setArgsInput('');
        setArgs([]);
        setUrl('');
        setEnvPairs([]);
    };

    const addArg = () => {
        if (argsInput.trim()) {
            setArgs(prev => [...prev, argsInput.trim()]);
            setArgsInput('');
        }
    };

    const handleSubmit = (e: React.FormEvent) => {
        e.preventDefault();
        const env: Record<string, string> = {};
        envPairs.filter(p => p.key).forEach(p => { env[p.key] = p.value; });

        addMutation.mutate({
            name,
            command: transport === 'command' ? command : undefined,
            args: transport === 'command' ? args : undefined,
            env: transport === 'command' ? env : undefined,
            url: transport === 'url' ? url : null,
        });
    };

    return (
        <Dialog open={open} onOpenChange={v => { onOpenChange(v); if (!v) reset(); }}>
            <DialogContent className="max-w-md">
                <DialogHeader>
                    <DialogTitle>Add MCP Server</DialogTitle>
                    <DialogDescription>
                        Configure a new MCP server for Claude to use as tools.
                    </DialogDescription>
                </DialogHeader>

                <form onSubmit={handleSubmit} className="px-5 py-4 space-y-4">
                    <div className="space-y-1.5">
                        <Label>Name</Label>
                        <Input
                            value={name}
                            onChange={e => setName(e.target.value)}
                            placeholder="e.g. filesystem"
                            required
                        />
                    </div>

                    <div className="space-y-1.5">
                        <Label>Transport</Label>
                        <SegmentedControl
                            type="single"
                            value={transport}
                            onValueChange={v => v && setTransport(v as TransportType)}
                        >
                            <SegmentedControlItem value="command">Command</SegmentedControlItem>
                            <SegmentedControlItem value="url">URL (HTTP/SSE)</SegmentedControlItem>
                        </SegmentedControl>
                    </div>

                    {transport === 'command' ? (
                        <>
                            <div className="space-y-1.5">
                                <Label>Command</Label>
                                <Input
                                    value={command}
                                    onChange={e => setCommand(e.target.value)}
                                    placeholder="npx @modelcontextprotocol/server-filesystem"
                                    required
                                    className="font-mono text-xs"
                                />
                            </div>

                            <div className="space-y-1.5">
                                <Label>Arguments <span className="text-zinc-600 font-normal">(optional)</span></Label>
                                {args.length > 0 && (
                                    <div className="flex flex-wrap gap-1 mb-1.5">
                                        {args.map((arg, i) => (
                                            <span
                                                key={i}
                                                className="inline-flex items-center gap-1 px-2 py-0.5 rounded-md bg-zinc-800 border border-zinc-700/60 text-xs text-zinc-300 font-mono"
                                            >
                                                {arg}
                                                <button
                                                    type="button"
                                                    onClick={() => setArgs(prev => prev.filter((_, j) => j !== i))}
                                                    className="text-zinc-600 hover:text-zinc-300 transition-colors"
                                                >
                                                    <X size={10} />
                                                </button>
                                            </span>
                                        ))}
                                    </div>
                                )}
                                <div className="flex gap-2">
                                    <Input
                                        value={argsInput}
                                        onChange={e => setArgsInput(e.target.value)}
                                        onKeyDown={e => e.key === 'Enter' && (e.preventDefault(), addArg())}
                                        placeholder="/path/to/directory"
                                        className="font-mono text-xs"
                                    />
                                    <Button type="button" variant="outline" size="icon" onClick={addArg}>
                                        <Plus size={14} />
                                    </Button>
                                </div>
                            </div>

                            <div className="space-y-1.5">
                                <div className="flex items-center justify-between">
                                    <Label>Environment variables <span className="text-zinc-600 font-normal">(optional)</span></Label>
                                    <button
                                        type="button"
                                        onClick={() => setEnvPairs(prev => [...prev, { key: '', value: '' }])}
                                        className="text-xs text-zinc-500 hover:text-zinc-300 transition-colors flex items-center gap-1"
                                    >
                                        <Plus size={10} /> Add
                                    </button>
                                </div>
                                {envPairs.map((pair, i) => (
                                    <div key={i} className="flex gap-2 items-center">
                                        <Input
                                            value={pair.key}
                                            onChange={e => setEnvPairs(prev => prev.map((p, j) => j === i ? { ...p, key: e.target.value } : p))}
                                            placeholder="KEY"
                                            className="font-mono text-xs w-32 shrink-0"
                                        />
                                        <Input
                                            value={pair.value}
                                            onChange={e => setEnvPairs(prev => prev.map((p, j) => j === i ? { ...p, value: e.target.value } : p))}
                                            placeholder="value"
                                            className="text-xs"
                                        />
                                        <button
                                            type="button"
                                            onClick={() => setEnvPairs(prev => prev.filter((_, j) => j !== i))}
                                            className="text-zinc-600 hover:text-zinc-300 transition-colors shrink-0"
                                        >
                                            <X size={14} />
                                        </button>
                                    </div>
                                ))}
                            </div>
                        </>
                    ) : (
                        <div className="space-y-1.5">
                            <Label>Server URL</Label>
                            <Input
                                value={url}
                                onChange={e => setUrl(e.target.value)}
                                placeholder="https://mcp.example.com/sse"
                                type="url"
                                required
                            />
                        </div>
                    )}
                </form>

                <DialogFooter>
                    <Button variant="ghost" onClick={() => onOpenChange(false)}>Cancel</Button>
                    <Button onClick={handleSubmit} disabled={addMutation.isPending || !name}>
                        {addMutation.isPending ? 'Adding…' : 'Add Server'}
                    </Button>
                </DialogFooter>
            </DialogContent>
        </Dialog>
    );
}
