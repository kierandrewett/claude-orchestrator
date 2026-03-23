import { useState, useRef, useEffect } from 'react';
import { Send, Square, Mic, MicOff } from 'lucide-react';
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
            setText((prev) => prev + (prev ? ' ' : '') + transcript);
        };
        r.onend = () => setIsListening(false);
        r.start();
        recognitionRef.current = r;
        setIsListening(true);
    };

    useEffect(() => {
        const ta = textareaRef.current;
        if (!ta) return;
        ta.style.height = 'auto';
        ta.style.height = Math.min(ta.scrollHeight, 180) + 'px';
    }, [text]);

    return (
        <div className="relative border-t border-zinc-800/80 bg-zinc-950 px-3 py-2.5">
            <div className="flex items-end gap-2 bg-zinc-900 border border-zinc-700/50 rounded-2xl px-3 py-2.5 focus-within:border-zinc-600 transition-colors">
                <textarea
                    ref={textareaRef}
                    value={text}
                    onChange={(e) => setText(e.target.value)}
                    onKeyDown={handleKeyDown}
                    placeholder="Message Claude…"
                    rows={1}
                    className="flex-1 bg-transparent text-sm text-zinc-100 placeholder:text-zinc-600 focus:outline-none resize-none font-sans leading-relaxed"
                    style={{ minHeight: '24px', maxHeight: '180px', fontSize: '16px' }}
                />
                <div className="flex items-center gap-1 shrink-0 pb-0.5">
                    <button
                        onClick={toggleVoice}
                        className={cn('p-1.5 rounded-lg transition-colors', isListening ? 'text-red-400 bg-red-500/10 animate-pulse' : 'text-zinc-600 hover:text-zinc-400')}
                        title="Voice input"
                    >
                        {isListening ? <MicOff className="w-4 h-4" /> : <Mic className="w-4 h-4" />}
                    </button>
                    <button
                        onClick={onStop}
                        className="p-1.5 text-zinc-600 hover:text-red-400 rounded-lg transition-colors"
                        title="Stop task"
                    >
                        <Square className="w-4 h-4" />
                    </button>
                    <button
                        onClick={send}
                        disabled={!text.trim() || sendMutation.isPending}
                        className="p-1.5 bg-zinc-700 hover:bg-zinc-600 disabled:bg-transparent disabled:text-zinc-700 text-zinc-200 rounded-lg transition-colors"
                        title="Send (Enter)"
                    >
                        <Send className="w-4 h-4" />
                    </button>
                </div>
            </div>
        </div>
    );
}
