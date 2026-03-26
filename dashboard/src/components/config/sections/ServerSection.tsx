import * as React from 'react';
import { FolderOpen, Server, KeyRound } from 'lucide-react';
import { Input } from '../../ui/input';
import { Button } from '../../ui/button';
import { Label } from '../../ui/label';
import { trpc } from '../../../api/trpc';
import { EnvBadge, isEnvValue } from '../EnvBadge';
import type { RawServerConfig } from '../../../server-types';

interface ServerSectionProps {
    config: RawServerConfig;
}

export function ServerSection({ config }: ServerSectionProps) {
    const [stateDir, setStateDir] = React.useState(config.state_dir ?? '');
    const [clientBind, setClientBind] = React.useState(config.client_bind ?? '');
    const [clientToken, setClientToken] = React.useState(isEnvValue(config.client_token) ? config.client_token : config.client_token ?? '');
    const [systemPrompt, setSystemPrompt] = React.useState(config.system_prompt ?? '');
    const [saved, setSaved] = React.useState(false);

    const updateSection = trpc.config.updateSection.useMutation({
        onSuccess: () => {
            setSaved(true);
            setTimeout(() => setSaved(false), 2000);
        },
    });

    const handleSave = () => {
        updateSection.mutate({
            section: 'server',
            values: {
                state_dir: stateDir || undefined,
                client_bind: clientBind || undefined,
                client_token: clientToken || undefined,
                system_prompt: systemPrompt || undefined,
            },
        });
    };

    return (
        <div className="rounded-xl border border-zinc-800 bg-zinc-900 overflow-hidden">
            <div className="px-4 py-3 border-b border-zinc-800 flex items-center gap-2">
                <Server className="w-4 h-4 text-zinc-400" />
                <h3 className="text-sm font-semibold text-zinc-200">Server</h3>
            </div>
            <div className="divide-y divide-zinc-800/60">
                <div className="flex items-center justify-between py-3 px-4">
                    <div className="flex-1 min-w-0 pr-4">
                        <Label>State Directory</Label>
                        <p className="text-xs text-zinc-500 mt-0.5">Where tasks and state are stored</p>
                    </div>
                    <div className="w-64 shrink-0">
                        <Input
                            value={stateDir}
                            onChange={e => setStateDir(e.target.value)}
                            icon={<FolderOpen size={12} />}
                            placeholder="~/.local/share/claude-orchestrator"
                        />
                    </div>
                </div>

                <div className="flex items-center justify-between py-3 px-4">
                    <div className="flex-1 min-w-0 pr-4">
                        <Label>Client Bind Address</Label>
                        <p className="text-xs text-zinc-500 mt-0.5">WebSocket address for client connections</p>
                    </div>
                    <div className="w-64 shrink-0">
                        <Input
                            value={clientBind}
                            onChange={e => setClientBind(e.target.value)}
                            placeholder="0.0.0.0:8765"
                        />
                    </div>
                </div>

                <div className="flex items-center justify-between py-3 px-4">
                    <div className="flex-1 min-w-0 pr-4">
                        <Label>Client Token</Label>
                        <p className="text-xs text-zinc-500 mt-0.5">Bearer token for client connections</p>
                    </div>
                    <div className="w-64 shrink-0">
                        {isEnvValue(config.client_token) ? (
                            <EnvBadge value={config.client_token} />
                        ) : (
                            <Input
                                type="password"
                                value={clientToken}
                                onChange={e => setClientToken(e.target.value)}
                                icon={<KeyRound size={12} />}
                                placeholder="Optional"
                            />
                        )}
                    </div>
                </div>

                <div className="flex items-start justify-between py-3 px-4">
                    <div className="flex-1 min-w-0 pr-4">
                        <Label>System Prompt</Label>
                        <p className="text-xs text-zinc-500 mt-0.5">Injected into every new session</p>
                    </div>
                    <div className="w-64 shrink-0">
                        <textarea
                            value={systemPrompt}
                            onChange={e => setSystemPrompt(e.target.value)}
                            className="w-full h-20 bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-100 placeholder:text-zinc-500 focus:outline-none focus:ring-2 focus:ring-zinc-500 resize-none"
                            placeholder="Optional system prompt..."
                        />
                    </div>
                </div>
            </div>

            <div className="px-4 py-3 border-t border-zinc-800 flex items-center justify-end gap-2">
                {saved && <span className="text-xs text-emerald-400">Saved!</span>}
                <Button onClick={handleSave} disabled={updateSection.isPending} size="sm">
                    {updateSection.isPending ? 'Saving...' : 'Save'}
                </Button>
            </div>
        </div>
    );
}
