export type SessionStatus = 'pending' | 'running' | 'completed' | 'failed' | 'killed';

export interface SessionStats {
    input_tokens: number;
    output_tokens: number;
    tool_calls: Record<string, number>;
    turns: number;
    cost_usd?: number;
    stop_reason?: string;
}

export interface SessionInfo {
    id: string;
    name?: string;
    cwd: string;
    status: SessionStatus;
    created_at: string;
    started_at?: string;
    ended_at?: string;
    stats: SessionStats;
    client_hostname?: string;
}

export interface SlashCommand {
    name: string;
    description: string;
}

// Raw Claude NDJSON event - pass through without schema
export type ClaudeEvent = Record<string, unknown>;

// Discriminated union for all server→dashboard messages
export type S2DMessage =
    | { type: 'session_list'; sessions: SessionInfo[] }
    | { type: 'session_created'; session: SessionInfo }
    | { type: 'session_updated'; session: SessionInfo }
    | { type: 'session_event'; session_id: string; event: ClaudeEvent }
    | { type: 'session_ended'; session_id: string; stats: SessionStats; exit_code: number }
    | { type: 'session_history'; session_id: string; events: ClaudeEvent[] }
    | { type: 'client_status'; connected: boolean; hostname: string | null }
    | { type: 'command_list'; commands: SlashCommand[] }
    | { type: 'error'; message: string };

export type D2SMessage =
    | { type: 'create_session'; name?: string; initial_prompt?: string }
    | { type: 'send_input'; session_id: string; text: string }
    | { type: 'kill_session'; session_id: string }
    | { type: 'get_history'; session_id: string };
