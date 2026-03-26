import * as React from 'react';
import { Send, KeyRound } from 'lucide-react';
import { Switch } from '../../ui/switch';
import { Input } from '../../ui/input';
import { Button } from '../../ui/button';
import { Label } from '../../ui/label';
import { trpc } from '../../../api/trpc';
import { EnvBadge, isEnvValue } from '../EnvBadge';
import type { RawTelegramConfig } from '../../../server-types';

interface TelegramSectionProps {
    config: RawTelegramConfig;
}

export function TelegramSection({ config }: TelegramSectionProps) {
    const [enabled, setEnabled] = React.useState(config.enabled ?? false);
    const [botToken, setBotToken] = React.useState(isEnvValue(config.bot_token) ? config.bot_token : config.bot_token ?? '');
    const [supergroupId, setSupergroupId] = React.useState(String(config.supergroup_id ?? ''));
    const [scratchpadTopic, setScratchpadTopic] = React.useState(config.scratchpad_topic_name ?? '');
    const [allowedUsers, setAllowedUsers] = React.useState(
        (config.allowed_users ?? []).join(', ')
    );
    const [saved, setSaved] = React.useState(false);

    const updateSection = trpc.config.updateSection.useMutation({
        onSuccess: () => {
            setSaved(true);
            setTimeout(() => setSaved(false), 2000);
        },
    });

    const handleSave = () => {
        const users = allowedUsers
            .split(',')
            .map((s: string) => parseInt(s.trim()))
            .filter((n: number) => !isNaN(n));

        updateSection.mutate({
            section: 'backends.telegram',
            values: {
                enabled,
                bot_token: isEnvValue(config.bot_token) ? config.bot_token : botToken || undefined,
                supergroup_id: parseInt(supergroupId) || undefined,
                scratchpad_topic_name: scratchpadTopic || undefined,
                allowed_users: users.length > 0 ? users : undefined,
            },
        });
    };

    return (
        <div className="rounded-xl border border-zinc-800 bg-zinc-900 overflow-hidden">
            <div className="px-4 py-3 border-b border-zinc-800 flex items-center gap-2">
                <Send className="w-4 h-4 text-zinc-400" />
                <h3 className="text-sm font-semibold text-zinc-200">Telegram Backend</h3>
            </div>
            <div className="divide-y divide-zinc-800/60">
                <div className="flex items-center justify-between py-3 px-4">
                    <div className="flex-1 min-w-0 pr-4">
                        <p className="text-sm font-medium text-zinc-200">Enabled</p>
                    </div>
                    <Switch checked={enabled} onCheckedChange={setEnabled} />
                </div>

                <div className="flex items-center justify-between py-3 px-4">
                    <div className="flex-1 min-w-0 pr-4">
                        <Label>Bot Token</Label>
                    </div>
                    <div className="w-64 shrink-0">
                        {isEnvValue(config.bot_token) ? (
                            <EnvBadge value={config.bot_token} />
                        ) : (
                            <Input
                                type="password"
                                value={botToken}
                                onChange={e => setBotToken(e.target.value)}
                                icon={<KeyRound size={12} />}
                                placeholder="1234567890:ABC..."
                            />
                        )}
                    </div>
                </div>

                <div className="flex items-center justify-between py-3 px-4">
                    <div className="flex-1 min-w-0 pr-4">
                        <Label>Supergroup ID</Label>
                        <p className="text-xs text-zinc-500 mt-0.5">Telegram group chat ID</p>
                    </div>
                    <div className="w-48 shrink-0">
                        <Input
                            value={supergroupId}
                            onChange={e => setSupergroupId(e.target.value)}
                            placeholder="-100123456789"
                        />
                    </div>
                </div>

                <div className="flex items-center justify-between py-3 px-4">
                    <div className="flex-1 min-w-0 pr-4">
                        <Label>Scratchpad Topic</Label>
                        <p className="text-xs text-zinc-500 mt-0.5">Topic name for the scratchpad</p>
                    </div>
                    <div className="w-48 shrink-0">
                        <Input
                            value={scratchpadTopic}
                            onChange={e => setScratchpadTopic(e.target.value)}
                            placeholder="scratchpad"
                        />
                    </div>
                </div>

                <div className="flex items-center justify-between py-3 px-4">
                    <div className="flex-1 min-w-0 pr-4">
                        <Label>Allowed Users</Label>
                        <p className="text-xs text-zinc-500 mt-0.5">Comma-separated Telegram user IDs</p>
                    </div>
                    <div className="w-64 shrink-0">
                        <Input
                            value={allowedUsers}
                            onChange={e => setAllowedUsers(e.target.value)}
                            placeholder="123456, 789012"
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
