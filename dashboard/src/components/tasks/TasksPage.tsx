import * as React from 'react';
import { Plus, LayoutGrid, List, Search, SlidersHorizontal } from 'lucide-react';
import { trpc } from '../../api/trpc';
import { TaskCard } from './TaskCard';
import { NewTaskDialog } from './NewTaskDialog';
import { Button } from '../ui/button';
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
        .filter(t =>
            !search ||
            t.name.toLowerCase().includes(search.toLowerCase()) ||
            t.profile.toLowerCase().includes(search.toLowerCase())
        )
        .sort((a, b) => {
            switch (sort) {
                case 'newest': return new Date(b.created_at).getTime() - new Date(a.created_at).getTime();
                case 'oldest': return new Date(a.created_at).getTime() - new Date(b.created_at).getTime();
                case 'name':   return a.name.localeCompare(b.name);
                case 'cost':   return b.cost_usd - a.cost_usd;
            }
        });

    const counts = {
        all:        tasks.length,
        Running:    tasks.filter(t => t.state === 'Running').length,
        Hibernated: tasks.filter(t => t.state === 'Hibernated').length,
        Dead:       tasks.filter(t => t.state === 'Dead').length,
    };

    const filterDefs: { key: FilterState; label: string; dot?: string }[] = [
        { key: 'all',        label: 'All' },
        { key: 'Running',    label: 'Running',    dot: 'bg-emerald-400' },
        { key: 'Hibernated', label: 'Hibernated', dot: 'bg-amber-400' },
        { key: 'Dead',       label: 'Finished',   dot: 'bg-zinc-500' },
    ];

    return (
        <div className="flex flex-col h-full overflow-hidden">
            {/* Page header */}
            <div className="flex items-center gap-3 px-4 md:px-6 pt-5 pb-4 border-b border-zinc-800/60 shrink-0">
                <div className="flex-1 min-w-0">
                    <h1 className="text-sm font-semibold text-zinc-100">Tasks</h1>
                    <p className="text-xs text-zinc-600 mt-0.5">{tasks.length} total</p>
                </div>
                <Button onClick={() => setNewTaskOpen(true)} size="sm">
                    <Plus size={13} />
                    New Task
                </Button>
            </div>

            {/* Controls */}
            <div className="flex items-center gap-2 px-4 md:px-6 py-3 border-b border-zinc-800/40 shrink-0 flex-wrap">
                {/* Filter pills */}
                <div className="flex items-center gap-1 flex-wrap">
                    {filterDefs.map(({ key, label, dot }) => (
                        <button
                            key={key}
                            onClick={() => setFilter(key)}
                            className={cn(
                                'flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs font-medium transition-colors border',
                                filter === key
                                    ? 'bg-zinc-800 text-zinc-100 border-zinc-700'
                                    : 'text-zinc-500 border-transparent hover:text-zinc-300 hover:bg-zinc-800/50',
                            )}
                        >
                            {dot && <span className={cn('w-1.5 h-1.5 rounded-full shrink-0', dot)} />}
                            {label}
                            <span className={cn(
                                'text-[10px] tabular-nums',
                                filter === key ? 'text-zinc-400' : 'text-zinc-600',
                            )}>
                                {counts[key]}
                            </span>
                        </button>
                    ))}
                </div>

                <div className="flex-1" />

                {/* Search */}
                <div className="relative">
                    <Search size={11} className="absolute left-2.5 top-1/2 -translate-y-1/2 text-zinc-600 pointer-events-none" />
                    <input
                        type="text"
                        placeholder="Search tasks..."
                        value={search}
                        onChange={e => setSearch(e.target.value)}
                        className="h-8 w-36 pl-7 pr-3 text-xs bg-zinc-900 border border-zinc-800 rounded-lg text-zinc-300 placeholder:text-zinc-600 focus:outline-none focus:ring-1 focus:ring-zinc-700 focus:border-zinc-700 transition-all focus:w-44"
                    />
                </div>

                {/* Sort */}
                <div className="relative">
                    <SlidersHorizontal size={11} className="absolute left-2.5 top-1/2 -translate-y-1/2 text-zinc-600 pointer-events-none" />
                    <select
                        value={sort}
                        onChange={e => setSort(e.target.value as SortKey)}
                        className="h-8 pl-7 pr-7 text-xs bg-zinc-900 border border-zinc-800 rounded-lg text-zinc-400 focus:outline-none focus:ring-1 focus:ring-zinc-700 appearance-none cursor-pointer"
                    >
                        <option value="newest">Newest</option>
                        <option value="oldest">Oldest</option>
                        <option value="name">Name</option>
                        <option value="cost">Cost</option>
                    </select>
                </div>

                {/* View toggle */}
                <div className="flex items-center gap-0.5 bg-zinc-900 border border-zinc-800 rounded-lg p-0.5">
                    <button
                        onClick={() => setView('grid')}
                        className={cn(
                            'p-1.5 rounded-md transition-colors',
                            view === 'grid' ? 'bg-zinc-700 text-zinc-100' : 'text-zinc-600 hover:text-zinc-400',
                        )}
                        title="Grid view"
                    >
                        <LayoutGrid size={13} />
                    </button>
                    <button
                        onClick={() => setView('list')}
                        className={cn(
                            'p-1.5 rounded-md transition-colors',
                            view === 'list' ? 'bg-zinc-700 text-zinc-100' : 'text-zinc-600 hover:text-zinc-400',
                        )}
                        title="List view"
                    >
                        <List size={13} />
                    </button>
                </div>
            </div>

            {/* Task list */}
            <div className="flex-1 overflow-auto px-4 md:px-6 py-4">
                {tasksQuery.isLoading ? (
                    <div className={cn(
                        view === 'grid'
                            ? 'grid grid-cols-1 sm:grid-cols-2 xl:grid-cols-3 gap-3'
                            : 'space-y-1.5',
                    )}>
                        {[...Array(6)].map((_, i) => (
                            <Skeleton key={i} className={view === 'grid' ? 'h-32 rounded-xl' : 'h-12 rounded-lg'} />
                        ))}
                    </div>
                ) : filtered.length === 0 ? (
                    <div className="flex flex-col items-center justify-center py-20 text-center">
                        <div className="w-12 h-12 rounded-xl bg-zinc-900 border border-zinc-800 flex items-center justify-center mb-4">
                            <LayoutGrid size={20} className="text-zinc-700" />
                        </div>
                        <p className="text-sm font-medium text-zinc-500">No tasks found</p>
                        <p className="text-xs text-zinc-700 mt-1">
                            {search
                                ? 'Try a different search term'
                                : filter !== 'all'
                                    ? `No ${filter.toLowerCase()} tasks`
                                    : 'Create a task to get started'}
                        </p>
                        {!search && filter === 'all' && (
                            <button
                                onClick={() => setNewTaskOpen(true)}
                                className="mt-4 flex items-center gap-1.5 text-xs text-zinc-400 hover:text-zinc-200 border border-zinc-800 hover:border-zinc-700 px-3 py-1.5 rounded-lg transition-colors"
                            >
                                <Plus size={12} /> New Task
                            </button>
                        )}
                    </div>
                ) : view === 'grid' ? (
                    <div className="grid grid-cols-1 sm:grid-cols-2 xl:grid-cols-3 gap-3">
                        {filtered.map(task => (
                            <TaskCard key={task.id} task={task} view="grid" />
                        ))}
                    </div>
                ) : (
                    <div className="rounded-xl border border-zinc-800/80 overflow-hidden bg-zinc-900/30">
                        {filtered.map((task, i) => (
                            <TaskCard
                                key={task.id}
                                task={task}
                                view="list"
                                isLast={i === filtered.length - 1}
                            />
                        ))}
                    </div>
                )}
            </div>

            <NewTaskDialog open={newTaskOpen} onOpenChange={setNewTaskOpen} />
        </div>
    );
}
