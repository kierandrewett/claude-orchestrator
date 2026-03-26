import * as React from 'react';
import { Globe, KeyRound, Link } from 'lucide-react';
import { Switch } from '../../ui/switch';
import { Input } from '../../ui/input';
import { Button } from '../../ui/button';
import { Label } from '../../ui/label';
import { trpc } from '../../../api/trpc';
import { EnvBadge, isEnvValue } from '../EnvBadge';
import type { RawWebConfig } from '../../../server-types';

interface WebSectionProps {
    config: RawWebConfig;
}

export function WebSection({ config }: WebSectionProps) {
    const [enabled, setEnabled] = React.useState(config.enabled ?? false);
    const [bind, setBind] = React.useState(config.bind ?? '');
    const [dashboardBind, setDashboardBind] = React.useState(config.dashboard_bind ?? '');
    const [dashboardToken, setDashboardToken] = React.useState(isEnvValue(config.dashboard_token) ? config.dashboard_token : config.dashboard_token ?? '');
    const [dashboardUrl, setDashboardUrl] = React.useState(config.dashboard_url ?? '');
    const [saved, setSaved] = React.useState(false);

    const updateSection = trpc.config.updateSection.useMutation({
        onSuccess: () => {
            setSaved(true);
            setTimeout(() => setSaved(false), 2000);
        },
    });

    const handleSave = () => {
        updateSection.mutate({
            section: 'backends.web',
            values: {
                enabled,
                bind: bind || undefined,
                dashboard_bind: dashboardBind || undefined,
                dashboard_token: dashboardToken || undefined,
                dashboard_url: dashboardUrl || undefined,
            },
        });
    };

    return (
        <div className="rounded-xl border border-zinc-800 bg-zinc-900 overflow-hidden">
            <div className="px-4 py-3 border-b border-zinc-800 flex items-center gap-2">
                <Globe className="w-4 h-4 text-zinc-400" />
                <h3 className="text-sm font-semibold text-zinc-200">Web Backend</h3>
            </div>
            <div className="divide-y divide-zinc-800/60">
                <div className="flex items-center justify-between py-3 px-4">
                    <div className="flex-1 min-w-0 pr-4">
                        <p className="text-sm font-medium text-zinc-200">Enabled</p>
                        <p className="text-xs text-zinc-500 mt-0.5">Enable the REST API and WebSocket server</p>
                    </div>
                    <Switch checked={enabled} onCheckedChange={setEnabled} />
                </div>

                <div className="flex items-center justify-between py-3 px-4">
                    <div className="flex-1 min-w-0 pr-4">
                        <Label>API Bind Address</Label>
                        <p className="text-xs text-zinc-500 mt-0.5">Rust API server address</p>
                    </div>
                    <div className="w-48 shrink-0">
                        <Input value={bind} onChange={e => setBind(e.target.value)} placeholder="0.0.0.0:8080" />
                    </div>
                </div>

                <div className="flex items-center justify-between py-3 px-4">
                    <div className="flex-1 min-w-0 pr-4">
                        <Label>Dashboard Bind</Label>
                        <p className="text-xs text-zinc-500 mt-0.5">Node.js dashboard server address</p>
                    </div>
                    <div className="w-48 shrink-0">
                        <Input value={dashboardBind} onChange={e => setDashboardBind(e.target.value)} placeholder="0.0.0.0:3001" />
                    </div>
                </div>

                <div className="flex items-center justify-between py-3 px-4">
                    <div className="flex-1 min-w-0 pr-4">
                        <Label>Dashboard Token</Label>
                        <p className="text-xs text-zinc-500 mt-0.5">Bearer token for dashboard access</p>
                    </div>
                    <div className="w-64 shrink-0">
                        {isEnvValue(config.dashboard_token) ? (
                            <EnvBadge value={config.dashboard_token} />
                        ) : (
                            <Input
                                type="password"
                                value={dashboardToken}
                                onChange={e => setDashboardToken(e.target.value)}
                                icon={<KeyRound size={12} />}
                                placeholder="Optional"
                            />
                        )}
                    </div>
                </div>

                <div className="flex items-center justify-between py-3 px-4">
                    <div className="flex-1 min-w-0 pr-4">
                        <Label>Dashboard URL</Label>
                        <p className="text-xs text-zinc-500 mt-0.5">External URL shown in Telegram button</p>
                    </div>
                    <div className="w-64 shrink-0">
                        <Input
                            value={dashboardUrl}
                            onChange={e => setDashboardUrl(e.target.value)}
                            icon={<Link size={12} />}
                            placeholder="https://dashboard.example.com"
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
