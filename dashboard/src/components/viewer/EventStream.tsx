import { useEffect, useRef, useMemo, useState } from 'react';
import { ChevronDown } from 'lucide-react';
import type { OrchestratorEvent } from '../../types';
import { EventRow } from './EventRow';

// ─── Types ────────────────────────────────────────────────────────────────────

export interface TextBlock {
    type: 'text';
    text: string;
    done: boolean;
}

export interface ToolUseBlock {
    type: 'tool_use';
    id: string;
    name: string;
    input: Record<string, unknown>;
    inputJson: string;
    inputStreaming: boolean;
    done: boolean;
}

export interface ToolResultBlock {
    type: 'tool_result';
    tool_use_id: string;
    content: string;
    is_error: boolean;
}

export type ContentBlock = TextBlock | ToolUseBlock | ToolResultBlock;

export interface ConversationTurn {
    role: 'assistant' | 'user';
    blocks: ContentBlock[];
}

// ─── Event accumulator ────────────────────────────────────────────────────────

function accumulateEvents(events: OrchestratorEvent[]): ConversationTurn[] {
    const turns: ConversationTurn[] = [];
    let currentAssistantTurn: ConversationTurn | null = null;

    const ensureAssistantTurn = (): ConversationTurn => {
        if (!currentAssistantTurn) {
            currentAssistantTurn = { role: 'assistant', blocks: [] };
            turns.push(currentAssistantTurn);
        }
        return currentAssistantTurn;
    };

    for (const event of events) {
        if ('TextOutput' in event) {
            const { text, is_continuation } = event.TextOutput;
            const turn = ensureAssistantTurn();
            if (is_continuation && turn.blocks.length > 0) {
                const last = turn.blocks[turn.blocks.length - 1];
                if (last?.type === 'text') { last.text += text; continue; }
            }
            turn.blocks.push({ type: 'text', text, done: false });
            continue;
        }
        if ('ToolStarted' in event) {
            const { tool_name, summary } = event.ToolStarted;
            ensureAssistantTurn().blocks.push({
                type: 'tool_use', id: tool_name, name: tool_name,
                input: { _summary: summary }, inputJson: summary,
                inputStreaming: false, done: false,
            });
            continue;
        }
        if ('ToolCompleted' in event) {
            const { tool_name, summary, is_error, output_preview } = event.ToolCompleted;
            if (currentAssistantTurn) {
                const turn = currentAssistantTurn as ConversationTurn;
                const block = [...turn.blocks].reverse()
                    .find(b => b.type === 'tool_use' && (b as ToolUseBlock).name === tool_name && !(b as ToolUseBlock).done);
                if (block?.type === 'tool_use') block.done = true;
            }
            turns.push({
                role: 'user',
                blocks: [{ type: 'tool_result', tool_use_id: tool_name, content: output_preview ?? summary, is_error }],
            });
            continue;
        }
        if ('TurnComplete' in event) {
            if (currentAssistantTurn) {
                const turn = currentAssistantTurn as ConversationTurn;
                for (const block of turn.blocks) {
                    if (block.type === 'text') block.done = true;
                }
            }
            currentAssistantTurn = null;
            continue;
        }
        if ('Error' in event) {
            ensureAssistantTurn().blocks.push({ type: 'text', text: `❌ ${event.Error.error}`, done: true });
            currentAssistantTurn = null;
            continue;
        }
        if ('CommandResponse' in event) {
            ensureAssistantTurn().blocks.push({ type: 'text', text: event.CommandResponse.text, done: true });
            currentAssistantTurn = null;
            continue;
        }
    }
    return turns;
}

// ─── EventStream ──────────────────────────────────────────────────────────────

interface EventStreamProps {
    events: OrchestratorEvent[];
}

export function EventStream({ events }: EventStreamProps) {
    const containerRef = useRef<HTMLDivElement>(null);
    const sentinelRef = useRef<HTMLDivElement>(null);
    const [userScrolled, setUserScrolled] = useState(false);
    const prevEventsLen = useRef(0);

    const turns = useMemo(() => accumulateEvents(events), [events]);

    // Auto-scroll when new events arrive
    useEffect(() => {
        const newEvents = events.length > prevEventsLen.current;
        prevEventsLen.current = events.length;

        if (newEvents && !userScrolled && sentinelRef.current) {
            sentinelRef.current.scrollIntoView({ behavior: 'smooth', block: 'end' });
        }
    }, [events.length, userScrolled]);

    // Reset auto-scroll on initial load
    useEffect(() => {
        setUserScrolled(false);
        sentinelRef.current?.scrollIntoView({ block: 'end' });
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, []);

    const handleScroll = () => {
        const el = containerRef.current;
        if (!el) return;
        const distFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight;
        setUserScrolled(distFromBottom > 80);
    };

    if (events.length === 0) {
        return (
            <div className="flex-1 flex flex-col items-center justify-center gap-2 text-zinc-700">
                <div className="w-8 h-8 rounded-full border border-zinc-800 flex items-center justify-center animate-pulse">
                    <div className="w-2 h-2 rounded-full bg-zinc-700" />
                </div>
                <p className="text-sm text-zinc-600">Waiting for Claude…</p>
            </div>
        );
    }

    return (
        <div
            ref={containerRef}
            onScroll={handleScroll}
            className="flex-1 overflow-y-auto flex flex-col"
        >
            <div className="px-4 md:px-6 py-4 flex flex-col gap-1 max-w-3xl w-full mx-auto">
                {turns.map((turn, i) => (
                    <EventRow key={i} turn={turn} />
                ))}
                <div ref={sentinelRef} className="h-2 shrink-0" />
            </div>

            {/* Scroll-to-bottom FAB */}
            {userScrolled && (
                <button
                    onClick={() => {
                        setUserScrolled(false);
                        sentinelRef.current?.scrollIntoView({ behavior: 'smooth' });
                    }}
                    className="sticky bottom-4 self-center flex items-center gap-1.5 px-3 py-1.5 text-xs rounded-full bg-zinc-800 border border-zinc-700 text-zinc-400 hover:text-zinc-200 hover:bg-zinc-700 transition-colors shadow-lg"
                >
                    <ChevronDown size={13} />
                    Scroll to bottom
                </button>
            )}
        </div>
    );
}
