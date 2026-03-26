import * as React from 'react';
import { Container } from 'lucide-react';
import { Input } from '../../ui/input';
import { Button } from '../../ui/button';
import { Label } from '../../ui/label';
import { trpc } from '../../../api/trpc';
import type { RawDockerConfig } from '../../../server-types';

interface DockerSectionProps {
    config: RawDockerConfig;
}

export function DockerSection({ config }: DockerSectionProps) {
    const [idleTimeout, setIdleTimeout] = React.useState(String(config.idle_timeout_hours ?? 12));
    const [defaultProfile, setDefaultProfile] = React.useState(config.default_profile ?? 'base');
    const [imagePrefix, setImagePrefix] = React.useState(config.image_prefix ?? '');
    const [saved, setSaved] = React.useState(false);

    const updateSection = trpc.config.updateSection.useMutation({
        onSuccess: () => {
            setSaved(true);
            setTimeout(() => setSaved(false), 2000);
        },
    });

    const handleSave = () => {
        updateSection.mutate({
            section: 'docker',
            values: {
                idle_timeout_hours: parseInt(idleTimeout) || 12,
                default_profile: defaultProfile || undefined,
                image_prefix: imagePrefix || undefined,
            },
        });
    };

    return (
        <div className="rounded-xl border border-zinc-800 bg-zinc-900 overflow-hidden">
            <div className="px-4 py-3 border-b border-zinc-800 flex items-center gap-2">
                <Container className="w-4 h-4 text-zinc-400" />
                <h3 className="text-sm font-semibold text-zinc-200">Docker</h3>
            </div>
            <div className="divide-y divide-zinc-800/60">
                <div className="flex items-center justify-between py-3 px-4">
                    <div className="flex-1 min-w-0 pr-4">
                        <Label>Idle Timeout</Label>
                        <p className="text-xs text-zinc-500 mt-0.5">Hours before idle task hibernation</p>
                    </div>
                    <div className="w-32 shrink-0">
                        <Input
                            type="number"
                            value={idleTimeout}
                            onChange={e => setIdleTimeout(e.target.value)}
                            min="1"
                            max="168"
                        />
                    </div>
                </div>

                <div className="flex items-center justify-between py-3 px-4">
                    <div className="flex-1 min-w-0 pr-4">
                        <Label>Default Profile</Label>
                        <p className="text-xs text-zinc-500 mt-0.5">Profile used for new tasks</p>
                    </div>
                    <div className="w-48 shrink-0">
                        <Input
                            value={defaultProfile}
                            onChange={e => setDefaultProfile(e.target.value)}
                            placeholder="base"
                        />
                    </div>
                </div>

                <div className="flex items-center justify-between py-3 px-4">
                    <div className="flex-1 min-w-0 pr-4">
                        <Label>Image Prefix</Label>
                        <p className="text-xs text-zinc-500 mt-0.5">Docker image name prefix</p>
                    </div>
                    <div className="w-64 shrink-0">
                        <Input
                            value={imagePrefix}
                            onChange={e => setImagePrefix(e.target.value)}
                            placeholder="orchestrator/claude-code"
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
