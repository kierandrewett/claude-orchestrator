import { useState, useRef, useEffect } from 'react';
import { Square, Mic, MicOff, ArrowUp } from 'lucide-react';
import { useMutation } from '@tanstack/react-query';
import { sendInput } from '../../api/client';
import { cn } from '../../lib/utils';

interface Props {
    taskId: string;
    onStop: () => void;
}

export function InputBar({ taskId, onStop }: Props) {
    const [text, setText] = useState('');
    const [isListening, setIsListening] = useState(false);
    const textareaRef = useRef<HTMLTextAreaElement>(null);
    const recognitionRef = useRef<{ stop: () => void } | null>(null);

    const sendMutation = useMutation({
        mutationFn: (t: string) => sendInput(taskId, t),
    });

    const send = () => {
        const trimmed = text.trim();
        if (!trimmed || sendMutation.isPending) return;
        sendMutation.mutate(trimmed);
        setText('');
    };

    const handleKeyDown = (e: React.KeyboardEvent) => {
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
                onresult: ((e: { results: ArrayLike<ArrayLike<{ transcript: string }>> }) => void) | null;
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
            setText(prev => prev + (prev ? ' ' : '') + transcript);
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
        ta.style.height = Math.min(ta.scrollHeight, 200) + 'px';
    }, [text]);

    const canSend = text.trim().length > 0 && !sendMutation.isPending;

    return (
        <div className="border-t border-zinc-800/60 bg-zinc-950 px-3 pb-3 pt-2.5 shrink-0">
            <div className={cn(
                'flex items-end gap-2 bg-zinc-900 border rounded-2xl px-3.5 py-2.5 transition-all',
                'border-zinc-800/80 focus-within:border-zinc-700',
            )}>
                <textarea
                    ref={textareaRef}
                    value={text}
                    onChange={e => setText(e.target.value)}
                    onKeyDown={handleKeyDown}
                    placeholder="Message Claude…"
                    rows={1}
                    className="flex-1 bg-transparent text-sm text-zinc-100 placeholder:text-zinc-600 focus:outline-none resize-none font-sans leading-relaxed"
                    style={{ minHeight: '22px', maxHeight: '200px', fontSize: '15px' }}
                />
                <div className="flex items-center gap-1 shrink-0 pb-0.5">
                    <button
                        onClick={toggleVoice}
                        className={cn(
                            'p-1.5 rounded-lg transition-colors',
                            isListening
                                ? 'text-red-400 bg-red-500/10 animate-pulse'
                                : 'text-zinc-600 hover:text-zinc-400 hover:bg-zinc-800/60',
                        )}
                        title="Voice input"
                    >
                        {isListening ? <MicOff size={14} /> : <Mic size={14} />}
                    </button>
                    <button
                        onClick={onStop}
                        className="p-1.5 text-zinc-600 hover:text-red-400 hover:bg-zinc-800/60 rounded-lg transition-colors"
                        title="Stop task"
                    >
                        <Square size={14} />
                    </button>
                    <button
                        onClick={send}
                        disabled={!canSend}
                        className={cn(
                            'p-1.5 rounded-lg transition-all',
                            canSend
                                ? 'bg-zinc-100 hover:bg-white text-zinc-900'
                                : 'bg-zinc-800 text-zinc-700 cursor-not-allowed',
                        )}
                        title="Send (Enter)"
                    >
                        {sendMutation.isPending ? (
                            <div className="w-3.5 h-3.5 rounded-full border-2 border-zinc-600 border-t-transparent animate-spin" />
                        ) : (
                            <ArrowUp size={14} />
                        )}
                    </button>
                </div>
            </div>
            <p className="text-[10px] text-zinc-700 text-center mt-1.5">
                Enter to send · Shift+Enter for new line
            </p>
        </div>
    );
}
