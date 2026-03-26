import { RefreshCw } from 'lucide-react';
import { trpc } from '../../api/trpc';
import { ServerSection } from './sections/ServerSection';
import { DockerSection } from './sections/DockerSection';
import { DisplaySection } from './sections/DisplaySection';
import { WebSection } from './sections/WebSection';
import { TelegramSection } from './sections/TelegramSection';
import { Skeleton } from '../ui/skeleton';
import { Button } from '../ui/button';
import type { RawConfig } from '../../server-types';

export function ConfigPage() {
    const configQuery = trpc.config.get.useQuery(undefined, {
        staleTime: 30_000,
    });
    const utils = trpc.useUtils();

    const config = configQuery.data as RawConfig | undefined;

    const handleReload = () => {
        void utils.config.get.invalidate();
    };

    return (
        <div className="p-4 md:p-6 space-y-5 max-w-2xl">
            <div className="flex items-center gap-3">
                <div className="flex-1">
                    <h1 className="text-sm font-semibold text-zinc-100">Configuration</h1>
                    <p className="text-xs text-zinc-600 mt-0.5">Edit your orchestrator.toml settings</p>
                </div>
                <Button variant="outline" size="sm" onClick={handleReload} disabled={configQuery.isFetching}>
                    <RefreshCw size={12} className={configQuery.isFetching ? 'animate-spin' : ''} />
                    Reload
                </Button>
            </div>

            {configQuery.isLoading ? (
                <div className="space-y-3">
                    {[...Array(4)].map((_, i) => <Skeleton key={i} className="h-48 rounded-xl" />)}
                </div>
            ) : configQuery.isError ? (
                <div className="rounded-xl border border-red-500/20 bg-red-500/5 p-4">
                    <p className="text-sm text-red-400 font-medium">Failed to load config</p>
                    <p className="text-xs text-red-400/60 mt-1">{configQuery.error.message}</p>
                    <p className="text-xs text-red-500/40 mt-1">
                        Make sure CONFIG_PATH is set correctly on the server.
                    </p>
                </div>
            ) : config ? (
                <div className="space-y-3">
                    {config.server && <ServerSection config={config.server} />}
                    {config.docker && <DockerSection config={config.docker} />}
                    {config.display && <DisplaySection config={config.display} />}
                    {config.backends?.web && <WebSection config={config.backends.web} />}
                    {config.backends?.telegram && <TelegramSection config={config.backends.telegram} />}
                </div>
            ) : null}
        </div>
    );
}
