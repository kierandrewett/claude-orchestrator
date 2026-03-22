import { useState, useRef, useEffect } from 'react';
import { Send, Square, Mic, MicOff, ChevronRight } from 'lucide-react';
import { useMutation } from '@tanstack/react-query';
import { useQuery } from '@tanstack/react-query';
import { sendInput, fetchStatus } from '../../api/client';
import { cn } from '../../lib/utils';
import type { SlashCommand } from '../../types';

interface Props {
    sessionId: string;
    onKill: () => void;
    pending?: boolean;
}

export function InputBar({ sessionId, onKill, pending = false }: Props) {
    const { data: status } = useQuery({ queryKey: ['status'], queryFn: fetchStatus, staleTime: 0 });
    const commands: SlashCommand[] = status?.commands ?? [];

    const [text, setText] = useState('');
    const [isListening, setIsListening] = useState(false);
    const [showCommands, setShowCommands] = useState(false);
    const [selectedIdx, setSelectedIdx] = useState(0);
    const textareaRef = useRef<HTMLTextAreaElement>(null);
    const recognitionRef = useRef<{ stop: () => void } | null>(null);

    const sendMutation = useMutation({
        mutationFn: (t: string) => sendInput(sessionId, t),
    });

    const filteredCommands: SlashCommand[] = text.startsWith('/')
        ? commands
              .filter(
                  (c) =>
                      c.name.toLowerCase().startsWith(text.toLowerCase()) ||
                      c.name.toLowerCase().includes(text.slice(1).toLowerCase()),
              )
              .slice(0, 8)
        : [];

    useEffect(() => {
        setShowCommands(filteredCommands.length > 0);
        setSelectedIdx(0);
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [text]);

    const send = () => {
        const trimmed = text.trim();
        if (!trimmed || pending) return;
        sendMutation.mutate(trimmed);
        setText('');
        setShowCommands(false);
    };

    const applyCommand = (cmd: SlashCommand) => {
        setText(cmd.name + ' ');
        setShowCommands(false);
        textareaRef.current?.focus();
    };

    const handleKeyDown = (e: React.KeyboardEvent) => {
        if (showCommands) {
            if (e.key === 'ArrowDown') {
                e.preventDefault();
                setSelectedIdx((i) => Math.min(i + 1, filteredCommands.length - 1));
                return;
            }
            if (e.key === 'ArrowUp') {
                e.preventDefault();
                setSelectedIdx((i) => Math.max(i - 1, 0));
                return;
            }
            if (e.key === 'Tab') {
                e.preventDefault();
                if (filteredCommands[selectedIdx]) applyCommand(filteredCommands[selectedIdx]);
                return;
            }
            if (e.key === 'Enter' && filteredCommands[selectedIdx]) {
                e.preventDefault();
                applyCommand(filteredCommands[selectedIdx]);
                return;
            }
            if (e.key === 'Escape') {
                setShowCommands(false);
                return;
            }
        }

        if (e.key === 'Enter' && !e.shiftKey) {
            e.preventDefault();
            send();
        }
    };

    const toggleVoice = () => {
        if (isListening) {
            recognitionRef.current?.stop();
            setIsListening(false);
            return;
        }
        type SR = {
            new (): {
                continuous: boolean;
                interimResults: boolean;
                onresult:
                    | ((e: { results: ArrayLike<ArrayLike<{ transcript: string }>> }) => void)
                    | null;
                onend: (() => void) | null;
                start: () => void;
                stop: () => void;
            };
        };
        const w = window as unknown as Record<string, unknown>;
        const SRClass = (w['SpeechRecognition'] ?? w['webkitSpeechRecognition']) as SR | undefined;
        if (!SRClass) return;
        const r = new SRClass();
        r.continuous = false;
        r.interimResults = false;
        r.onresult = (e) => {
            const transcript = e.results[0]?.[0]?.transcript ?? '';
            setText((prev) => prev + (prev ? ' ' : '') + transcript);
        };
        r.onend = () => setIsListening(false);
        r.start();
        recognitionRef.current = r;
        setIsListening(true);
    };

    // Auto-resize textarea
    useEffect(() => {
        const ta = textareaRef.current;
        if (!ta) return;
        ta.style.height = 'auto';
        ta.style.height = Math.min(ta.scrollHeight, 180) + 'px';
    }, [text]);

    return (
        <div className="relative border-t border-zinc-800/80 bg-zinc-950 px-3 pt-3 pb-4">
            {/* Slash command menu */}
            {showCommands && (
                <div className="absolute bottom-full left-3 right-3 mb-2 bg-zinc-900 border border-zinc-700/50 rounded-xl shadow-2xl overflow-hidden z-10">
                    <div className="px-3 py-2 text-[10px] text-zinc-600 uppercase tracking-wider border-b border-zinc-800">
                        Commands
                    </div>
                    {filteredCommands.map((cmd, i) => (
                        <button
                            key={cmd.name}
                            onClick={() => applyCommand(cmd)}
                            className={cn(
                                'w-full flex items-center gap-3 px-3 py-2 text-left transition-colors',
                                i === selectedIdx ? 'bg-zinc-800' : 'hover:bg-zinc-800/60',
                            )}
                        >
                            <span className="font-mono text-sm text-emerald-400 shrink-0">
                                {cmd.name}
                            </span>
                            {cmd.description && (
                                <span className="text-xs text-zinc-500 truncate">
                                    {cmd.description}
                                </span>
                            )}
                            <ChevronRight className="w-3 h-3 text-zinc-700 ml-auto shrink-0" />
                        </button>
                    ))}
                </div>
            )}

            {/* Input area */}
            <div className="flex items-end gap-2 bg-zinc-900 border border-zinc-700/50 rounded-2xl px-3 py-2.5 focus-within:border-zinc-600 transition-colors">
                <textarea
                    ref={textareaRef}
                    value={text}
                    onChange={(e) => setText(e.target.value)}
                    onKeyDown={handleKeyDown}
                    placeholder={pending ? 'Starting session…' : 'Message Claude… (/ for commands)'}
                    rows={1}
                    disabled={pending}
                    className="flex-1 bg-transparent text-sm text-zinc-100 placeholder:text-zinc-600 focus:outline-none resize-none font-sans leading-relaxed disabled:opacity-50"
                    style={{ minHeight: '24px', maxHeight: '180px' }}
                />

                <div className="flex items-center gap-1 shrink-0 pb-0.5">
                    <button
                        onClick={toggleVoice}
                        className={cn(
                            'p-1.5 rounded-lg transition-colors',
                            isListening
                                ? 'text-red-400 bg-red-500/10 animate-pulse'
                                : 'text-zinc-600 hover:text-zinc-400',
                        )}
                        title="Voice input"
                    >
                        {isListening ? <MicOff className="w-4 h-4" /> : <Mic className="w-4 h-4" />}
                    </button>

                    <button
                        onClick={onKill}
                        className="p-1.5 text-zinc-600 hover:text-red-400 rounded-lg transition-colors"
                        title="Stop session"
                    >
                        <Square className="w-4 h-4" />
                    </button>

                    <button
                        onClick={send}
                        disabled={!text.trim() || pending || sendMutation.isPending}
                        className="p-1.5 bg-zinc-700 hover:bg-zinc-600 disabled:bg-transparent disabled:text-zinc-700 text-zinc-200 rounded-lg transition-colors"
                        title="Send (Enter)"
                    >
                        <Send className="w-4 h-4" />
                    </button>
                </div>
            </div>

            <p className="mt-1.5 text-[10px] text-zinc-700 text-center">
                Enter to send · Shift+Enter for newline · Tab to complete
            </p>
        </div>
    );
}
