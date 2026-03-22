import { useEffect, useRef, useMemo, useState } from 'react';
import type { ClaudeEvent } from '../../types';
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

function accumulateEvents(events: ClaudeEvent[]): ConversationTurn[] {
    const turns: ConversationTurn[] = [];

    // Current assistant turn being built
    let currentAssistantTurn: ConversationTurn | null = null;
    // Track current block index for streaming
    let currentBlockIndex = -1;

    const ensureAssistantTurn = (): ConversationTurn => {
        if (!currentAssistantTurn) {
            currentAssistantTurn = { role: 'assistant', blocks: [] };
            turns.push(currentAssistantTurn);
        }
        return currentAssistantTurn;
    };

    const finishAssistantTurn = () => {
        if (currentAssistantTurn) {
            // Mark all blocks as done
            for (const block of currentAssistantTurn.blocks) {
                if (block.type === 'text' || block.type === 'tool_use') {
                    block.done = true;
                }
            }
        }
        currentAssistantTurn = null;
        currentBlockIndex = -1;
    };

    for (const event of events) {
        const eventType = event['type'] as string | undefined;

        // ── Streaming format ──────────────────────────────────────────────────

        if (eventType === 'message_start') {
            finishAssistantTurn();
            ensureAssistantTurn();
            continue;
        }

        if (eventType === 'content_block_start') {
            const turn = ensureAssistantTurn();
            const contentBlock = event['content_block'] as Record<string, unknown> | undefined;
            const blockType = contentBlock?.['type'] as string | undefined;
            const blockIndex = (event['index'] as number | undefined) ?? turn.blocks.length;
            currentBlockIndex = blockIndex;

            if (blockType === 'text') {
                const block: ContentBlock = { type: 'text', text: '', done: false };
                turn.blocks[blockIndex] = block;
            } else if (blockType === 'tool_use') {
                const block: ContentBlock = {
                    type: 'tool_use',
                    id: (contentBlock?.['id'] as string | undefined) ?? '',
                    name: (contentBlock?.['name'] as string | undefined) ?? 'unknown',
                    input: {},
                    inputJson: '',
                    inputStreaming: false,
                    done: false,
                };
                turn.blocks[blockIndex] = block;
            }
            continue;
        }

        if (eventType === 'content_block_delta') {
            const turn = ensureAssistantTurn();
            const delta = event['delta'] as Record<string, unknown> | undefined;
            const deltaType = delta?.['type'] as string | undefined;
            const blockIndex = (event['index'] as number | undefined) ?? currentBlockIndex;
            const block = turn.blocks[blockIndex];

            if (!block) continue;

            if (deltaType === 'text_delta' && block.type === 'text') {
                block.text += (delta?.['text'] as string | undefined) ?? '';
            } else if (deltaType === 'input_json_delta' && block.type === 'tool_use') {
                block.inputJson += (delta?.['partial_json'] as string | undefined) ?? '';
                block.inputStreaming = true;
                try {
                    block.input = JSON.parse(block.inputJson) as Record<string, unknown>;
                } catch {
                    // partial JSON, ignore parse error
                }
            }
            continue;
        }

        if (eventType === 'content_block_stop') {
            const turn = currentAssistantTurn as ConversationTurn | null;
            if (!turn) continue;
            const blockIndex = (event['index'] as number | undefined) ?? currentBlockIndex;
            const block = turn.blocks[blockIndex];
            if (!block) continue;
            if (block.type === 'text' || block.type === 'tool_use') {
                block.done = true;
                if (block.type === 'tool_use') {
                    block.inputStreaming = false;
                    if (block.inputJson) {
                        try {
                            block.input = JSON.parse(block.inputJson) as Record<string, unknown>;
                        } catch {
                            // keep partial
                        }
                    }
                }
            }
            continue;
        }

        if (eventType === 'message_delta') {
            // Contains stop_reason etc — no rendering needed here, handled by session stats
            continue;
        }

        if (eventType === 'message_stop') {
            finishAssistantTurn();
            continue;
        }

        // ── Turn-complete format (assistant message) ───────────────────────────

        if (eventType === 'assistant') {
            finishAssistantTurn();
            const message = event['message'] as Record<string, unknown> | undefined;
            const content = (event['content'] ?? message?.['content']) as unknown[] | undefined;
            if (!content) continue;

            const assistantTurn: ConversationTurn = { role: 'assistant', blocks: [] };
            turns.push(assistantTurn);

            for (const item of content) {
                const c = item as Record<string, unknown>;
                const ctype = c['type'] as string | undefined;
                if (ctype === 'text') {
                    assistantTurn.blocks.push({
                        type: 'text',
                        text: (c['text'] as string | undefined) ?? '',
                        done: true,
                    });
                } else if (ctype === 'tool_use') {
                    const input = (c['input'] as Record<string, unknown> | undefined) ?? {};
                    assistantTurn.blocks.push({
                        type: 'tool_use',
                        id: (c['id'] as string | undefined) ?? '',
                        name: (c['name'] as string | undefined) ?? 'unknown',
                        input,
                        inputJson: JSON.stringify(input),
                        inputStreaming: false,
                        done: true,
                    });
                }
            }
            continue;
        }

        // ── Turn-complete format (user / tool results) ────────────────────────

        if (eventType === 'user') {
            const message = event['message'] as Record<string, unknown> | undefined;
            const rawContent = event['content'] ?? message?.['content'];

            let userText = '';
            const toolResults: ToolResultBlock[] = [];

            if (typeof rawContent === 'string') {
                userText = rawContent;
            } else if (Array.isArray(rawContent)) {
                for (const item of rawContent) {
                    const c = item as Record<string, unknown>;
                    if (c['type'] === 'tool_result') {
                        const rc = c['content'];
                        let resultText = '';
                        if (typeof rc === 'string') {
                            resultText = rc;
                        } else if (Array.isArray(rc)) {
                            resultText = (rc as Array<Record<string, unknown>>)
                                .filter((r) => r['type'] === 'text')
                                .map((r) => r['text'] as string)
                                .join('\n');
                        }
                        toolResults.push({
                            type: 'tool_result',
                            tool_use_id: (c['tool_use_id'] as string | undefined) ?? '',
                            content: resultText,
                            is_error: (c['is_error'] as boolean | undefined) ?? false,
                        });
                    } else if (c['type'] === 'text') {
                        userText += (c['text'] as string | undefined) ?? '';
                    }
                }
            }

            if (userText) {
                turns.push({ role: 'user', blocks: [{ type: 'text', text: userText, done: true }] });
            }
            if (toolResults.length > 0) {
                turns.push({ role: 'user', blocks: toolResults });
            }
            continue;
        }

        // ── result event ──────────────────────────────────────────────────────
        // (session-level summary — skip, handled by stats panel)
        if (eventType === 'result') {
            continue;
        }

        // ── Skip boring events ────────────────────────────────────────────────
        // ping, heartbeat, etc.
    }

    return turns;
}

// ─── EventStream ──────────────────────────────────────────────────────────────

interface EventStreamProps {
    events: ClaudeEvent[];
}

export function EventStream({ events }: EventStreamProps) {
    const containerRef = useRef<HTMLDivElement>(null);
    const sentinelRef = useRef<HTMLDivElement>(null);
    const [userScrolled, setUserScrolled] = useState(false);
    const prevEventsLen = useRef(0);

    const turns = useMemo(() => accumulateEvents(events), [events]);

    // Auto-scroll to bottom when new events arrive, unless user scrolled up
    useEffect(() => {
        const newEvents = events.length > prevEventsLen.current;
        prevEventsLen.current = events.length;

        if (newEvents && !userScrolled && sentinelRef.current) {
            sentinelRef.current.scrollIntoView({ behavior: 'smooth', block: 'end' });
        }
    }, [events.length, userScrolled]);

    // Reset auto-scroll when events are replaced (history load)
    useEffect(() => {
        setUserScrolled(false);
        if (sentinelRef.current) {
            sentinelRef.current.scrollIntoView({ block: 'end' });
        }
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
            <div className="flex-1 flex items-center justify-center text-zinc-600 text-sm">
                No events yet — waiting for Claude...
            </div>
        );
    }

    return (
        <div
            ref={containerRef}
            onScroll={handleScroll}
            className="flex-1 overflow-y-auto px-4 py-4 flex flex-col gap-4"
        >
            {turns.map((turn, i) => (
                <EventRow key={i} turn={turn} />
            ))}

            {/* Sentinel for auto-scroll */}
            <div ref={sentinelRef} className="h-px shrink-0" />

            {/* Scroll-to-bottom button */}
            {userScrolled && (
                <button
                    onClick={() => {
                        setUserScrolled(false);
                        sentinelRef.current?.scrollIntoView({ behavior: 'smooth' });
                    }}
                    className="sticky bottom-4 self-center px-3 py-1.5 text-xs rounded-full bg-zinc-800 border border-zinc-700 text-zinc-400 hover:text-zinc-200 hover:bg-zinc-700 transition-colors shadow-lg"
                >
                    Scroll to bottom ↓
                </button>
            )}
        </div>
    );
}
