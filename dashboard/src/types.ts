// Task state matching Rust TaskStateSummary
export type TaskState = 'Running' | 'Hibernated' | 'Dead';

// Task info maintained client-side from orchestrator events
export interface TaskInfo {
    id: string;
    name: string;
    profile: string;
    state: TaskState;
    created_at: string;
    // accumulated from TurnComplete events
    input_tokens: number;
    output_tokens: number;
    cost_usd: number;
    turns: number;
    last_activity?: string;
}

// UsageStats matching Rust UsageStats
export interface UsageStats {
    input_tokens: number;
    output_tokens: number;
    cache_read_tokens: number;
    cache_creation_tokens: number;
    total_cost_usd: number;
    turns: number;
}

// OrchestratorEvent — Rust serde externally-tagged enum
// Each variant is serialized as { VariantName: { fields... } }
export type OrchestratorEvent =
    | { TextOutput: { task_id: string; text: string; is_continuation: boolean } }
    | { ToolStarted: { task_id: string; tool_name: string; summary: string } }
    | { ToolCompleted: { task_id: string; tool_name: string; summary: string; is_error: boolean; output_preview: string | null } }
    | { Thinking: { task_id: string; text: string } }
    | { TurnComplete: { task_id: string; usage: UsageStats; duration_secs: number } }
    | { TaskCreated: { task_id: string; name: string; profile: string; kind: string } }
    | { TaskStateChanged: { task_id: string; old_state: string; new_state: TaskState } }
    | { Error: { task_id: string | null; error: string; next_steps: string[] } }
    | { QueuedMessageDelivered: { task_id: string } }
    | { FileOutput: { task_id: string; filename: string; mime_type: string | null; caption: string | null } }
    | { CommandResponse: { task_id: string | null; text: string } }
    | { PhaseChanged: { task_id: string; phase: string; trigger_message: unknown } };
