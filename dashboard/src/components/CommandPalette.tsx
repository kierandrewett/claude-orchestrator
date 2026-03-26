import { useNavigate } from '@tanstack/react-router';
import { LayoutDashboard, Terminal, Plug, Clock, Settings2, Plus, LogIn } from 'lucide-react';
import * as DialogPrimitive from '@radix-ui/react-dialog';
import {
    Command,
    CommandInput,
    CommandList,
    CommandEmpty,
    CommandGroup,
    CommandItem,
    CommandSeparator,
    CommandShortcut,
} from './ui/command';
import { trpc } from '../api/trpc';
import { cn } from '../lib/utils';

interface CommandPaletteProps {
    open: boolean;
    onOpenChange: (open: boolean) => void;
}

export function CommandPalette({ open, onOpenChange }: CommandPaletteProps) {
    const navigate = useNavigate();
    const tasksQuery = trpc.tasks.list.useQuery(undefined, { enabled: open });
    const createTask = trpc.tasks.create.useMutation();

    const runAction = (fn: () => void) => {
        fn();
        onOpenChange(false);
    };

    const pages = [
        { label: 'Overview', to: '/', icon: LayoutDashboard, shortcut: 'G O' },
        { label: 'Tasks', to: '/tasks', icon: Terminal, shortcut: 'G T' },
        { label: 'MCP Servers', to: '/mcp', icon: Plug, shortcut: 'G M' },
        { label: 'Scheduled Events', to: '/scheduler', icon: Clock, shortcut: 'G S' },
        { label: 'Configuration', to: '/config', icon: Settings2, shortcut: 'G C' },
    ];

    return (
        <DialogPrimitive.Root open={open} onOpenChange={onOpenChange}>
            <DialogPrimitive.Portal>
                <DialogPrimitive.Overlay
                    className="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0"
                />
                <DialogPrimitive.Content
                    className={cn(
                        'fixed left-1/2 top-1/4 z-50 -translate-x-1/2 w-full max-w-xl',
                        'rounded-xl border border-zinc-700 bg-zinc-900 shadow-2xl',
                        'data-[state=open]:animate-in data-[state=closed]:animate-out',
                        'data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0',
                        'data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95',
                        'data-[state=open]:slide-in-from-top-4',
                    )}
                >
                    <DialogPrimitive.Title className="sr-only">Command palette</DialogPrimitive.Title>
                    <Command>
                        <CommandInput placeholder="Search actions, pages, tasks..." />
                        <CommandList className="max-h-80">
                            <CommandEmpty>No results found.</CommandEmpty>

                            <CommandGroup heading="Actions">
                                <CommandItem
                                    onSelect={() => runAction(() => createTask.mutate({}))}
                                >
                                    <Plus size={14} className="text-zinc-400" />
                                    <span>New Task</span>
                                    <CommandShortcut>N</CommandShortcut>
                                </CommandItem>
                                <CommandItem
                                    onSelect={() => runAction(() => void navigate({ to: '/login' }))}
                                >
                                    <LogIn size={14} className="text-zinc-400" />
                                    <span>Change Token</span>
                                </CommandItem>
                            </CommandGroup>

                            <CommandSeparator />

                            <CommandGroup heading="Navigate">
                                {pages.map(({ label, to, icon: Icon, shortcut }) => (
                                    <CommandItem
                                        key={to}
                                        onSelect={() => runAction(() => void navigate({ to }))}
                                    >
                                        <Icon size={14} className="text-zinc-400" />
                                        <span>{label}</span>
                                        <CommandShortcut>{shortcut}</CommandShortcut>
                                    </CommandItem>
                                ))}
                            </CommandGroup>

                            {tasksQuery.data && tasksQuery.data.length > 0 && (
                                <>
                                    <CommandSeparator />
                                    <CommandGroup heading="Tasks">
                                        {tasksQuery.data.slice(0, 5).map((task) => (
                                            <CommandItem
                                                key={task.id}
                                                onSelect={() => runAction(() => void navigate({ to: '/tasks/$id', params: { id: task.id } }))}
                                            >
                                                <span
                                                    className={cn(
                                                        'w-1.5 h-1.5 rounded-full shrink-0',
                                                        task.state === 'Running' ? 'bg-emerald-400' :
                                                        task.state === 'Hibernated' ? 'bg-amber-400' : 'bg-zinc-600',
                                                    )}
                                                />
                                                <span className="truncate">{task.name}</span>
                                                <span className="text-zinc-500 text-xs shrink-0">{task.profile}</span>
                                            </CommandItem>
                                        ))}
                                    </CommandGroup>
                                </>
                            )}
                        </CommandList>
                    </Command>
                </DialogPrimitive.Content>
            </DialogPrimitive.Portal>
        </DialogPrimitive.Root>
    );
}
