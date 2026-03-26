import * as React from 'react';
import { Plus, LayoutGrid, List, Search } from 'lucide-react';
import { trpc } from '../../api/trpc';
import { TaskCard } from './TaskCard';
import { NewTaskDialog } from './NewTaskDialog';
import { Button } from '../ui/button';
import { Input } from '../ui/input';
import { Skeleton } from '../ui/skeleton';
import { cn } from '../../lib/utils';
import type { TaskState } from '../../server-types';

type FilterState = 'all' | TaskState;
type SortKey = 'newest' | 'oldest' | 'name' | 'cost';
type ViewMode = 'grid' | 'list';

export function TasksPage() {
    const [filter, setFilter] = React.useState<FilterState>('all');
    const [sort, setSort] = React.useState<SortKey>('newest');
    const [view, setView] = React.useState<ViewMode>('grid');
    const [search, setSearch] = React.useState('');
    const [newTaskOpen, setNewTaskOpen] = React.useState(false);

    const tasksQuery = trpc.tasks.list.useQuery(undefined, { refetchInterval: 3000 });
    const tasks = tasksQuery.data ?? [];

    const filtered = tasks
        .filter(t => filter === 'all' || t.state === filter)
        .filter(t => !search || t.name.toLowerCase().includes(search.toLowerCase()) || t.profile.toLowerCase().includes(search.toLowerCase()))
        .sort((a, b) => {
            switch (sort) {
                case 'newest': return new Date(b.created_at).getTime() - new Date(a.created_at).getTime();
                case 'oldest': return new Date(a.created_at).getTime() - new Date(b.created_at).getTime();
                case 'name': return a.name.localeCompare(b.name);
                case 'cost': return b.cost_usd - a.cost_usd;
            }
        });

    const counts = {
        all: tasks.length,
        Running: tasks.filter(t => t.state === 'Running').length,
        Hibernated: tasks.filter(t => t.state === 'Hibernated').length,
        Dead: tasks.filter(t => t.state === 'Dead').length,
    };

    const filters: { key: FilterState; label: string }[] = [
        { key: 'all', label: `All (${counts.all})` },
        { key: 'Running', label: `Running (${counts.Running})` },
        { key: 'Hibernated', label: `Hibernated (${counts.Hibernated})` },
        { key: 'Dead', label: `Dead (${counts.Dead})` },
    ];

    return (
        <div className="p-6 space-y-4">
            {/* Header */}
            <div className="flex items-center gap-3 flex-wrap">
                <div className="flex-1 min-w-0">
                    <h1 className="text-lg font-semibold text-zinc-100">Tasks</h1>
                    <p className="text-sm text-zinc-500">{tasks.length} total</p>
                </div>
                <Button onClick={() => setNewTaskOpen(true)} size="sm">
                    <Plus size={13} />
                    New Task
                </Button>
            </div>

            {/* Controls */}
            <div className="flex items-center gap-2 flex-wrap">
                {/* Filter chips */}
                <div className="flex items-center gap-1">
                    {filters.map(({ key, label }) => (
                        <button
                            key={key}
                            onClick={() => setFilter(key)}
                            className={cn(
                                'px-2.5 py-1 rounded-lg text-xs font-medium transition-colors',
                                filter === key
                                    ? 'bg-zinc-700 text-zinc-100'
                                    : 'text-zinc-500 hover:text-zinc-300 hover:bg-zinc-800',
                            )}
                        >
                            {label}
                        </button>
                    ))}
                </div>

                <div className="flex-1" />

                {/* Search */}
                <div className="w-40">
                    <Input
                        placeholder="Search..."
                        value={search}
                        onChange={e => setSearch(e.target.value)}
                        icon={<Search size={12} />}
                    />
                </div>

                {/* Sort */}
                <select
                    value={sort}
                    onChange={e => setSort(e.target.value as SortKey)}
                    className="h-9 px-2 rounded-lg bg-zinc-800 border border-zinc-700 text-sm text-zinc-200 focus:outline-none focus:ring-2 focus:ring-zinc-500"
                >
                    <option value="newest">Newest</option>
                    <option value="oldest">Oldest</option>
                    <option value="name">Name</option>
                    <option value="cost">Cost</option>
                </select>

                {/* View toggle */}
                <div className="flex items-center gap-0.5 bg-zinc-800 rounded-lg p-0.5">
                    <button
                        onClick={() => setView('grid')}
                        className={cn('p-1.5 rounded-md transition-colors', view === 'grid' ? 'bg-zinc-700 text-zinc-100' : 'text-zinc-500 hover:text-zinc-300')}
                    >
                        <LayoutGrid size={14} />
                    </button>
                    <button
                        onClick={() => setView('list')}
                        className={cn('p-1.5 rounded-md transition-colors', view === 'list' ? 'bg-zinc-700 text-zinc-100' : 'text-zinc-500 hover:text-zinc-300')}
                    >
                        <List size={14} />
                    </button>
                </div>
            </div>

            {/* Task grid/list */}
            {tasksQuery.isLoading ? (
                <div className={cn(view === 'grid' ? 'grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4' : 'rounded-xl border border-zinc-800 bg-zinc-900 overflow-hidden')}>
                    {[...Array(6)].map((_, i) => (
                        <Skeleton key={i} className="h-32 rounded-xl" />
                    ))}
                </div>
            ) : filtered.length === 0 ? (
                <div className="flex flex-col items-center justify-center py-16 text-zinc-600">
                    <p className="text-sm">No tasks found</p>
                    <p className="text-xs mt-1">
                        {search ? 'Try a different search' : 'Create a new task to get started'}
                    </p>
                </div>
            ) : view === 'grid' ? (
                <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
                    {filtered.map(task => (
                        <TaskCard key={task.id} task={task} view="grid" />
                    ))}
                </div>
            ) : (
                <div className="rounded-xl border border-zinc-800 bg-zinc-900 overflow-hidden">
                    {filtered.map(task => (
                        <TaskCard key={task.id} task={task} view="list" />
                    ))}
                </div>
            )}

            <NewTaskDialog open={newTaskOpen} onOpenChange={setNewTaskOpen} />
        </div>
    );
}
