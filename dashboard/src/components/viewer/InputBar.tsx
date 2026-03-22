import { useState, useRef, useEffect } from 'react';
import { Send, Square, Mic, MicOff, ChevronRight } from 'lucide-react';
import { useSessionsStore, useCommands } from '../../store/sessions';
import { cn } from '../../lib/utils';
import type { SlashCommand } from '../../types';

interface Props {
    sessionId: string;
    onKill: () => void;
}

export function InputBar({ sessionId, onKill }: Props) {
    const { sendInput } = useSessionsStore();
    const commands = useCommands();
    const [text, setText] = useState('');
    const [isListening, setIsListening] = useState(false);
    const [showCommands, setShowCommands] = useState(false);
    const [selectedIdx, setSelectedIdx] = useState(0);
    const textareaRef = useRef<HTMLTextAreaElement>(null);
    const recognitionRef = useRef<any>(null);

    // Filter commands based on current text
    const filteredCommands: SlashCommand[] = text.startsWith('/')
        ? commands.filter(c =>
            c.name.toLowerCase().startsWith(text.toLowerCase()) ||
            c.name.toLowerCase().includes(text.slice(1).toLowerCase())
          ).slice(0, 8)
        : [];

    useEffect(() => {
        setShowCommands(filteredCommands.length > 0);
        setSelectedIdx(0);
    }, [text]);

    const send = () => {
        const trimmed = text.trim();
        if (!trimmed) return;
        sendInput(sessionId, trimmed);
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
                setSelectedIdx(i => Math.min(i + 1, filteredCommands.length - 1));
                return;
            }
            if (e.key === 'ArrowUp') {
                e.preventDefault();
                setSelectedIdx(i => Math.max(i - 1, 0));
                return;
            }
            if (e.key === 'Tab' || e.key === 'Enter' && filteredCommands[selectedIdx]) {
                if (e.key === 'Enter' && !e.shiftKey && filteredCommands[selectedIdx]) {
                    e.preventDefault();
                    applyCommand(filteredCommands[selectedIdx]);
                    return;
                }
                if (e.key === 'Tab') {
                    e.preventDefault();
                    applyCommand(filteredCommands[selectedIdx]);
                    return;
                }
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
        const SR = (window as any).SpeechRecognition || (window as any).webkitSpeechRecognition;
        if (!SR) return;
        const r = new SR();
        r.continuous = false;
        r.interimResults = false;
        r.onresult = (e: any) => {
            const transcript = e.results[0][0].transcript;
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
        ta.style.height = Math.min(ta.scrollHeight, 160) + 'px';
    }, [text]);

    return (
        <div className="relative border-t border-zinc-800 bg-zinc-950 p-3">
            {/* Slash command dropdown */}
            {showCommands && (
                <div className="absolute bottom-full left-3 right-3 mb-2 bg-zinc-900 border border-zinc-700 rounded-lg shadow-xl overflow-hidden z-10">
                    <div className="px-3 py-1.5 text-[10px] text-zinc-600 uppercase tracking-wider border-b border-zinc-800">
                        Slash commands
                    </div>
                    {filteredCommands.map((cmd, i) => (
                        <button
                            key={cmd.name}
                            onClick={() => applyCommand(cmd)}
                            className={cn(
                                'w-full flex items-center gap-3 px-3 py-2 text-left hover:bg-zinc-800 transition-colors',
                                i === selectedIdx && 'bg-zinc-800'
                            )}
                        >
                            <span className="font-mono text-sm text-emerald-400 flex-shrink-0">{cmd.name}</span>
                            {cmd.description && (
                                <span className="text-xs text-zinc-500 truncate">{cmd.description}</span>
                            )}
                            <ChevronRight className="w-3 h-3 text-zinc-700 ml-auto flex-shrink-0" />
                        </button>
                    ))}
                </div>
            )}

            <div className="flex items-end gap-2">
                <textarea
                    ref={textareaRef}
                    value={text}
                    onChange={e => setText(e.target.value)}
                    onKeyDown={handleKeyDown}
                    placeholder="Message Claude… (/ for commands, Enter to send)"
                    rows={1}
                    className="flex-1 bg-zinc-800/80 border border-zinc-700 rounded-lg px-3 py-2.5 text-sm text-zinc-100 placeholder:text-zinc-600 focus:outline-none focus:border-zinc-600 resize-none font-mono leading-relaxed transition-colors"
                    style={{ minHeight: '42px', maxHeight: '160px' }}
                />
                <div className="flex gap-1.5 flex-shrink-0">
                    <button
                        onClick={toggleVoice}
                        className={cn(
                            'p-2.5 rounded-lg transition-colors',
                            isListening
                                ? 'bg-red-500/20 text-red-400 animate-pulse'
                                : 'bg-zinc-800 text-zinc-500 hover:text-zinc-300'
                        )}
                        title="Voice input"
                    >
                        {isListening ? <MicOff className="w-4 h-4" /> : <Mic className="w-4 h-4" />}
                    </button>
                    <button
                        onClick={send}
                        disabled={!text.trim()}
                        className="p-2.5 bg-emerald-600 hover:bg-emerald-500 disabled:bg-zinc-800 disabled:text-zinc-600 text-white rounded-lg transition-colors"
                        title="Send (Enter)"
                    >
                        <Send className="w-4 h-4" />
                    </button>
                    <button
                        onClick={onKill}
                        className="p-2.5 bg-zinc-800 hover:bg-red-900/50 text-zinc-500 hover:text-red-400 rounded-lg transition-colors"
                        title="Stop session"
                    >
                        <Square className="w-4 h-4" />
                    </button>
                </div>
            </div>
            <p className="mt-1.5 text-[10px] text-zinc-700">
                Enter to send · Shift+Enter for newline · Tab to complete command
            </p>
        </div>
    );
}
