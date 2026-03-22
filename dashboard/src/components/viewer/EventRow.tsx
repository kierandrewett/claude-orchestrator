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
    CheckCircle,
} from 'lucide-react';
import { useState } from 'react';
import { cn } from '../../lib/utils';
import type { ConversationTurn, TextBlock, ToolUseBlock, ToolResultBlock } from './EventStream';

// Tool name → Lucide icon
function ToolIcon({ name }: { name: string }) {
    const icons: Record<string, React.ComponentType<{ className?: string }>> = {
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
    return <Icon className="w-3.5 h-3.5" />;
}

// Get the most meaningful arg from tool input for the header summary
function getToolSummary(name: string, input: Record<string, unknown>): string {
    const key = {
        Read: 'file_path',
        Write: 'file_path',
        Edit: 'file_path',
        MultiEdit: 'file_path',
        Glob: 'pattern',
        Grep: 'pattern',
        Bash: 'command',
        WebFetch: 'url',
        WebSearch: 'query',
    }[name];
    if (!key) return '';
    const val = input[key];
    if (typeof val !== 'string') return '';
    // Shorten: show only filename for paths
    if (key === 'file_path') return val.replace(/.*[\\/]/, '');
    // Truncate long commands
    return val.length > 60 ? val.slice(0, 57) + '…' : val;
}

function TextBlockView({ block }: { block: TextBlock }) {
    if (!block.text) return null;
    return (
        <div className="prose prose-invert max-w-none text-sm text-zinc-200 leading-relaxed">
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
        block.inputStreaming
            ? block.inputJson // show raw partial JSON while streaming
            : JSON.stringify(block.input, null, 2)
    }\n\`\`\``;

    return (
        <div className="rounded-md border border-zinc-800 bg-zinc-900/50 overflow-hidden text-sm">
            <button
                onClick={() => setOpen((o) => !o)}
                className="w-full flex items-center gap-2 px-3 py-2 text-left hover:bg-zinc-800/50 transition-colors"
            >
                {open ? (
                    <ChevronDown className="w-3.5 h-3.5 text-zinc-500 flex-shrink-0" />
                ) : (
                    <ChevronRight className="w-3.5 h-3.5 text-zinc-500 flex-shrink-0" />
                )}
                <span className="text-zinc-400">
                    <ToolIcon name={block.name} />
                </span>
                <span className="font-mono text-emerald-400 font-medium">{block.name}</span>
                {summary && <span className="text-zinc-500 font-mono truncate">{summary}</span>}
                {block.inputStreaming && (
                    <span className="ml-auto text-[10px] text-amber-400 animate-pulse">
                        streaming…
                    </span>
                )}
            </button>
            {open && (
                <div className="border-t border-zinc-800 px-3 py-2">
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
    const preview =
        lines[0]?.slice(0, 80) + (lines.length > 1 || (lines[0]?.length ?? 0) > 80 ? '…' : '');

    return (
        <div
            className={cn(
                'rounded-md border text-sm overflow-hidden',
                block.is_error
                    ? 'border-red-900/50 bg-red-950/20'
                    : 'border-zinc-800 bg-zinc-900/30',
            )}
        >
            <button
                onClick={() => setOpen((o) => !o)}
                className="w-full flex items-center gap-2 px-3 py-1.5 text-left hover:bg-zinc-800/30 transition-colors"
            >
                {block.is_error ? (
                    <AlertCircle className="w-3.5 h-3.5 text-red-400 flex-shrink-0" />
                ) : (
                    <CheckCircle className="w-3.5 h-3.5 text-zinc-600 flex-shrink-0" />
                )}
                <span
                    className={cn(
                        'font-mono truncate text-xs',
                        block.is_error ? 'text-red-400' : 'text-zinc-500',
                    )}
                >
                    {preview || '(empty)'}
                </span>
                {open ? (
                    <ChevronDown className="w-3 h-3 text-zinc-600 flex-shrink-0 ml-auto" />
                ) : (
                    <ChevronRight className="w-3 h-3 text-zinc-600 flex-shrink-0 ml-auto" />
                )}
            </button>
            {open && (
                <div className="border-t border-zinc-800/50 px-3 py-2">
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
            <div className="flex flex-col gap-2 py-3">
                <div className="flex items-center gap-2 text-[11px] text-zinc-600 uppercase tracking-wider font-medium">
                    <span className="w-4 h-px bg-zinc-800" />
                    Claude
                    <span className="flex-1 h-px bg-zinc-800/50" />
                </div>
                <div className="flex flex-col gap-2 pl-2">
                    {turn.blocks.map((block, i) => {
                        if (block.type === 'text') return <TextBlockView key={i} block={block} />;
                        if (block.type === 'tool_use')
                            return <ToolUseBlockView key={i} block={block} />;
                        if (block.type === 'tool_result')
                            return <ToolResultBlockView key={i} block={block} />;
                        return null;
                    })}
                </div>
            </div>
        );
    }

    // User turn (tool results only shown above, actual user messages are plain)
    return (
        <div className="flex flex-col gap-2 py-2">
            <div className="flex items-center gap-2 text-[11px] text-zinc-600 uppercase tracking-wider font-medium">
                <span className="w-4 h-px bg-zinc-800" />
                You
                <span className="flex-1 h-px bg-zinc-800/50" />
            </div>
            <div className="pl-2 flex flex-col gap-2">
                {turn.blocks.map((block, i) => {
                    if (block.type === 'text') return <TextBlockView key={i} block={block} />;
                    if (block.type === 'tool_result')
                        return <ToolResultBlockView key={i} block={block} />;
                    return null;
                })}
            </div>
        </div>
    );
}
