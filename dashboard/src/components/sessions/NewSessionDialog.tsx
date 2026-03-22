import * as Dialog from '@radix-ui/react-dialog';
import { useState } from 'react';
import { X } from 'lucide-react';
import { useSessionsStore } from '../../store/sessions';

interface Props {
    open: boolean;
    onOpenChange: (open: boolean) => void;
}

export function NewSessionDialog({ open, onOpenChange }: Props) {
    const { createSession, wsConnected } = useSessionsStore();
    const [name, setName] = useState('');
    const [prompt, setPrompt] = useState('');

    const handleSubmit = (e: React.FormEvent) => {
        e.preventDefault();
        createSession({
            name: name.trim() || undefined,
            initial_prompt: prompt.trim() || undefined,
        });
        setName('');
        setPrompt('');
        onOpenChange(false);
    };

    return (
        <Dialog.Root open={open} onOpenChange={onOpenChange}>
            <Dialog.Portal>
                <Dialog.Overlay className="fixed inset-0 bg-black/60 backdrop-blur-sm z-40" />
                <Dialog.Content className="fixed left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 z-50 w-full max-w-md bg-zinc-900 border border-zinc-800 rounded-xl shadow-2xl p-6">
                    <div className="flex items-center justify-between mb-6">
                        <Dialog.Title className="text-lg font-semibold text-zinc-100">
                            New Session
                        </Dialog.Title>
                        <Dialog.Close className="text-zinc-500 hover:text-zinc-300 transition-colors">
                            <X className="w-5 h-5" />
                        </Dialog.Close>
                    </div>

                    <form onSubmit={handleSubmit} className="flex flex-col gap-4">
                        <div className="flex flex-col gap-1.5">
                            <label className="text-sm text-zinc-400">Name <span className="text-zinc-600">(optional)</span></label>
                            <input
                                type="text"
                                value={name}
                                onChange={e => setName(e.target.value)}
                                placeholder="Untitled session"
                                className="bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-100 placeholder:text-zinc-600 focus:outline-none focus:border-zinc-500 transition-colors"
                            />
                        </div>

                        <div className="flex flex-col gap-1.5">
                            <label className="text-sm text-zinc-400">
                                Prompt <span className="text-zinc-600">(optional — you can send messages after starting)</span>
                            </label>
                            <textarea
                                value={prompt}
                                onChange={e => setPrompt(e.target.value)}
                                placeholder="What should Claude work on? Mention a file path or directory if needed."
                                rows={4}
                                autoFocus
                                className="bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-100 placeholder:text-zinc-600 focus:outline-none focus:border-zinc-500 transition-colors resize-none font-mono"
                                onKeyDown={e => {
                                    if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
                                        e.preventDefault();
                                        handleSubmit(e as any);
                                    }
                                }}
                            />
                            <p className="text-xs text-zinc-600">Ctrl+Enter to submit</p>
                        </div>

                        <button
                            type="submit"
                            disabled={!wsConnected}
                            className="mt-2 bg-emerald-600 hover:bg-emerald-500 disabled:bg-zinc-700 disabled:text-zinc-500 text-white rounded-lg px-4 py-2.5 text-sm font-medium transition-colors"
                        >
                            Start Session
                        </button>
                    </form>
                </Dialog.Content>
            </Dialog.Portal>
        </Dialog.Root>
    );
}
