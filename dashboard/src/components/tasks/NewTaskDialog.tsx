import * as React from 'react';
import {
    Dialog,
    DialogContent,
    DialogHeader,
    DialogTitle,
    DialogDescription,
    DialogFooter,
} from '../ui/dialog';
import {
    Select,
    SelectTrigger,
    SelectContent,
    SelectItem,
    SelectValue,
} from '../ui/select';
import { Button } from '../ui/button';
import { Label } from '../ui/label';
import { trpc } from '../../api/trpc';

interface NewTaskDialogProps {
    open: boolean;
    onOpenChange: (open: boolean) => void;
}

export function NewTaskDialog({ open, onOpenChange }: NewTaskDialogProps) {
    const [profile, setProfile] = React.useState('');
    const [prompt, setPrompt] = React.useState('');

    const profilesQuery = trpc.tasks.profiles.useQuery(undefined, { enabled: open });
    const profiles = profilesQuery.data?.profiles ?? [];

    const createMutation = trpc.tasks.create.useMutation({
        onSuccess: () => {
            onOpenChange(false);
            setProfile('');
            setPrompt('');
        },
    });

    const handleSubmit = (e: React.FormEvent) => {
        e.preventDefault();
        createMutation.mutate({
            profile: profile || undefined,
            prompt: prompt || undefined,
        });
    };

    return (
        <Dialog open={open} onOpenChange={onOpenChange}>
            <DialogContent className="max-w-md">
                <DialogHeader>
                    <DialogTitle>New Task</DialogTitle>
                    <DialogDescription>
                        Create a new Claude Code session.
                    </DialogDescription>
                </DialogHeader>

                <form onSubmit={handleSubmit} className="px-5 py-4 space-y-4">
                    <div className="space-y-1.5">
                        <Label htmlFor="profile">Profile</Label>
                        <Select value={profile} onValueChange={setProfile}>
                            <SelectTrigger id="profile">
                                <SelectValue placeholder="Default profile" />
                            </SelectTrigger>
                            <SelectContent>
                                {profiles.map(p => (
                                    <SelectItem key={p} value={p}>{p}</SelectItem>
                                ))}
                                {profiles.length === 0 && (
                                    <SelectItem value="_default">base</SelectItem>
                                )}
                            </SelectContent>
                        </Select>
                    </div>

                    <div className="space-y-1.5">
                        <Label htmlFor="prompt">Initial Prompt (optional)</Label>
                        <textarea
                            id="prompt"
                            className="w-full h-24 bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-100 placeholder:text-zinc-500 focus:outline-none focus:ring-2 focus:ring-zinc-500 resize-none"
                            placeholder="What would you like Claude to do?"
                            value={prompt}
                            onChange={e => setPrompt(e.target.value)}
                        />
                    </div>
                </form>

                <DialogFooter>
                    <Button variant="ghost" onClick={() => onOpenChange(false)}>
                        Cancel
                    </Button>
                    <Button
                        onClick={handleSubmit}
                        disabled={createMutation.isPending}
                    >
                        {createMutation.isPending ? 'Creating...' : 'Create Task'}
                    </Button>
                </DialogFooter>
            </DialogContent>
        </Dialog>
    );
}
