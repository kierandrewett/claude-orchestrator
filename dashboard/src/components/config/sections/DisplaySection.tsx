import * as React from 'react';
import { Eye } from 'lucide-react';
import { Switch } from '../../ui/switch';
import { Button } from '../../ui/button';
import { trpc } from '../../../api/trpc';
import type { RawDisplayConfig } from '../../../server-types';

interface DisplaySectionProps {
    config: RawDisplayConfig;
}

export function DisplaySection({ config }: DisplaySectionProps) {
    const [showThinking, setShowThinking] = React.useState(config.show_thinking ?? false);
    const [coalesceMs, setCoalesceMs] = React.useState(config.stream_coalesce_ms ?? 500);
    const [saved, setSaved] = React.useState(false);

    const updateSection = trpc.config.updateSection.useMutation({
        onSuccess: () => {
            setSaved(true);
            setTimeout(() => setSaved(false), 2000);
        },
    });

    const handleSave = () => {
        updateSection.mutate({
            section: 'display',
            values: {
                show_thinking: showThinking,
                stream_coalesce_ms: coalesceMs,
            },
        });
    };

    return (
        <div className="rounded-xl border border-zinc-800 bg-zinc-900 overflow-hidden">
            <div className="px-4 py-3 border-b border-zinc-800 flex items-center gap-2">
                <Eye className="w-4 h-4 text-zinc-400" />
                <h3 className="text-sm font-semibold text-zinc-200">Display</h3>
            </div>
            <div className="divide-y divide-zinc-800/60">
                <div className="flex items-center justify-between py-3 px-4">
                    <div className="flex-1 min-w-0 pr-4">
                        <p className="text-sm font-medium text-zinc-200">Show Thinking</p>
                        <p className="text-xs text-zinc-500 mt-0.5">Show Claude's internal thinking blocks</p>
                    </div>
                    <Switch checked={showThinking} onCheckedChange={setShowThinking} />
                </div>

                <div className="flex items-center justify-between py-3 px-4">
                    <div className="flex-1 min-w-0 pr-4">
                        <p className="text-sm font-medium text-zinc-200">Stream Coalesce</p>
                        <p className="text-xs text-zinc-500 mt-0.5">Debounce streaming messages by {coalesceMs} ms</p>
                    </div>
                    <div className="flex items-center gap-3 shrink-0">
                        <span className="text-xs tabular-nums text-zinc-400 w-14 text-right">{coalesceMs} ms</span>
                        <input
                            type="range"
                            min={100}
                            max={2000}
                            step={100}
                            value={coalesceMs}
                            onChange={e => setCoalesceMs(parseInt(e.target.value))}
                            className="w-32 accent-emerald-500"
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
