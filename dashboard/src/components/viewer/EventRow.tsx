import { Streamdown } from 'streamdown';
import type { CodeHighlighterPlugin } from 'streamdown';
import { code as _code } from '@streamdown/code';
const code = _code as unknown as CodeHighlighterPlugin;
import {
    ChevronDown,
    ChevronRight,
    FileSearch,
    FilePen,
    Terminal,
    Globe,
    Bot,
    Wrench,
    AlertCircle,
    CheckCircle2,
} from 'lucide-react';
import { useState } from 'react';
import { cn } from '../../lib/utils';
import type { ConversationTurn, TextBlock, ToolUseBlock, ToolResultBlock } from './EventStream';

// Tool name → icon
function ToolIcon({ name, className }: { name: string; className?: string }) {
    const icons: Record<string, React.ComponentType<{ className?: string; size?: number }>> = {
        Read: FileSearch,
        Glob: FileSearch,
        Grep: FileSearch,
        Write: FilePen,
        Edit: FilePen,
        MultiEdit: FilePen,
        Bash: Terminal,
        WebFetch: Globe,
        WebSearch: Globe,
        Agent: Bot,
    };
    const Icon = icons[name] ?? Wrench;
    return <Icon size={13} className={className} />;
}

function getToolSummary(name: string, input: Record<string, unknown>): string {
    const keyMap: Record<string, string> = {
        Read: 'file_path', Write: 'file_path', Edit: 'file_path', MultiEdit: 'file_path',
        Glob: 'pattern', Grep: 'pattern', Bash: 'command', WebFetch: 'url', WebSearch: 'query',
    };
    const key = keyMap[name];
    if (!key) return '';
    const val = input[key] ?? input['_summary'];
    if (typeof val !== 'string') return '';
    if (key === 'file_path') return val.replace(/.*[\\/]/, '');
    return val.length > 72 ? val.slice(0, 69) + '…' : val;
}

function TextBlockView({ block }: { block: TextBlock }) {
    if (!block.text) return null;
    return (
        <div className="prose prose-invert prose-sm max-w-none text-zinc-200 leading-relaxed">
            <Streamdown animated plugins={{ code }} isAnimating={!block.done}>
                {block.text}
            </Streamdown>
        </div>
    );
}

function ToolUseBlockView({ block }: { block: ToolUseBlock }) {
    const [open, setOpen] = useState(false);
    const summary = getToolSummary(block.name, block.input);
    const inputMarkdown = `\`\`\`json\n${
        block.inputStreaming ? block.inputJson : JSON.stringify(block.input, null, 2)
    }\n\`\`\``;

    return (
        <div className="rounded-lg border border-zinc-800/80 bg-zinc-900/60 overflow-hidden text-sm">
            <button
                onClick={() => setOpen(o => !o)}
                className="w-full flex items-center gap-2 px-3 py-2 text-left hover:bg-zinc-800/40 transition-colors"
            >
                <span className="text-zinc-600">
                    {open ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
                </span>
                <span className="text-zinc-500">
                    <ToolIcon name={block.name} />
                </span>
                <span className="font-mono text-[12px] font-semibold text-zinc-300">{block.name}</span>
                {summary && (
                    <span className="text-zinc-600 font-mono text-[11px] truncate">{summary}</span>
                )}
                {!block.done && !block.inputStreaming && (
                    <span className="ml-auto flex items-center gap-1 text-[10px] text-emerald-500">
                        <span className="w-1.5 h-1.5 rounded-full bg-emerald-500 animate-pulse" />
                        running
                    </span>
                )}
                {block.inputStreaming && (
                    <span className="ml-auto text-[10px] text-amber-400 animate-pulse">streaming…</span>
                )}
            </button>
            {open && (
                <div className="border-t border-zinc-800/60 px-3 py-2 bg-zinc-900/80">
                    <Streamdown animated plugins={{ code }} isAnimating={block.inputStreaming}>
                        {inputMarkdown}
                    </Streamdown>
                </div>
            )}
        </div>
    );
}

function ToolResultBlockView({ block }: { block: ToolResultBlock }) {
    const [open, setOpen] = useState(false);
    const lines = block.content.split('\n');
    const preview = lines[0]?.slice(0, 90) + (lines.length > 1 || (lines[0]?.length ?? 0) > 90 ? '…' : '');

    return (
        <div className={cn(
            'rounded-lg border text-sm overflow-hidden',
            block.is_error ? 'border-red-900/40 bg-red-950/15' : 'border-zinc-800/60 bg-zinc-900/30',
        )}>
            <button
                onClick={() => setOpen(o => !o)}
                className="w-full flex items-center gap-2 px-3 py-1.5 text-left hover:bg-zinc-800/20 transition-colors"
            >
                {block.is_error ? (
                    <AlertCircle size={12} className="text-red-400 shrink-0" />
                ) : (
                    <CheckCircle2 size={12} className="text-zinc-700 shrink-0" />
                )}
                <span className={cn('font-mono text-[11px] truncate flex-1 text-left', block.is_error ? 'text-red-400' : 'text-zinc-600')}>
                    {preview || '(empty)'}
                </span>
                {open ? (
                    <ChevronDown size={11} className="text-zinc-700 shrink-0" />
                ) : (
                    <ChevronRight size={11} className="text-zinc-700 shrink-0" />
                )}
            </button>
            {open && (
                <div className="border-t border-zinc-800/40 px-3 py-2">
                    <Streamdown animated plugins={{ code }} isAnimating={false}>
                        {`\`\`\`\n${block.content}\n\`\`\``}
                    </Streamdown>
                </div>
            )}
        </div>
    );
}

export function EventRow({ turn }: { turn: ConversationTurn }) {
    if (turn.role === 'assistant') {
        return (
            <div className="py-3">
                <div className="flex items-center gap-2 mb-2.5 text-[10px] text-zinc-700 uppercase tracking-widest font-semibold">
                    <div className="w-5 h-px bg-zinc-800" />
                    Claude
                    <div className="flex-1 h-px bg-zinc-800/50" />
                </div>
                <div className="flex flex-col gap-2 pl-1">
                    {turn.blocks.map((block, i) => {
                        if (block.type === 'text') return <TextBlockView key={i} block={block} />;
                        if (block.type === 'tool_use') return <ToolUseBlockView key={i} block={block} />;
                        if (block.type === 'tool_result') return <ToolResultBlockView key={i} block={block} />;
                        return null;
                    })}
                </div>
            </div>
        );
    }

    // User turn: text bubbles or tool results
    const hasOnlyText = turn.blocks.every(b => b.type === 'text');

    if (hasOnlyText) {
        return (
            <div className="flex justify-end py-2">
                <div className="max-w-[80%] bg-zinc-800 rounded-2xl rounded-br-md px-4 py-2.5 text-sm text-zinc-100">
                    {turn.blocks.map((block, i) => {
                        if (block.type === 'text') {
                            return (
                                <p key={i} className="whitespace-pre-wrap leading-relaxed">
                                    {block.text}
                                </p>
                            );
                        }
                        return null;
                    })}
                </div>
            </div>
        );
    }

    return (
        <div className="py-2">
            <div className="flex items-center gap-2 mb-2 text-[10px] text-zinc-700 uppercase tracking-widest font-semibold">
                <div className="w-5 h-px bg-zinc-800" />
                Tool Results
                <div className="flex-1 h-px bg-zinc-800/50" />
            </div>
            <div className="pl-1 flex flex-col gap-1.5">
                {turn.blocks.map((block, i) => {
                    if (block.type === 'text') return <TextBlockView key={i} block={block} />;
                    if (block.type === 'tool_result') return <ToolResultBlockView key={i} block={block} />;
                    return null;
                })}
            </div>
        </div>
    );
}
