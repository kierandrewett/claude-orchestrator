import { EventEmitter } from 'events';
import WebSocket from 'ws';

// ── Types ─────────────────────────────────────────────────────────────────────

export interface UsageStats {
    input_tokens: number;
    output_tokens: number;
    cache_read_tokens: number;
    cache_creation_tokens: number;
    total_cost_usd: number;
    turns: number;
}

export type TaskState = 'Running' | 'Hibernated' | 'Dead';

export interface TaskInfo {
    id: string;
    name: string;
    profile: string;
    state: TaskState;
    created_at: string;
    input_tokens: number;
    output_tokens: number;
    cost_usd: number;
    turns: number;
    last_activity: string;
}

export type OrchestratorEvent =
    | { TextOutput: { task_id: string; text: string; is_continuation: boolean } }
    | { ToolStarted: { task_id: string; tool_name: string; summary: string } }
    | { ToolCompleted: { task_id: string; tool_name: string; summary: string; is_error: boolean; output_preview: string | null } }
    | { Thinking: { task_id: string; text: string } }
    | { TurnComplete: { task_id: string; usage: UsageStats; duration_secs: number } }
    | { TaskCreated: { task_id: string; name: string; profile: string; kind: string } }
    | { TaskStateChanged: { task_id: string; old_state: string; new_state: TaskState } }
    | { ConversationRenamed: { task_id: string; title: string } }
    | { Error: { task_id: string | null; error: string; next_steps: string[] } }
    | { QueuedMessageDelivered: { task_id: string } }
    | { FileOutput: { task_id: string; filename: string; mime_type: string | null; caption: string | null } }
    | { CommandResponse: { task_id: string | null; text: string } }
    | { PhaseChanged: { task_id: string; phase: string } };

export interface EventLogEntry {
    id: number;
    event: OrchestratorEvent;
    timestamp: string;
}

export interface MetricsSummary {
    total_tasks: number;
    running_tasks: number;
    hibernated_tasks: number;
    dead_tasks: number;
    total_cost_usd: number;
    total_input_tokens: number;
    total_output_tokens: number;
    connected: boolean;
}

// ── State ─────────────────────────────────────────────────────────────────────

const ORCHESTRATOR_WS_URL = process.env.ORCHESTRATOR_WS_URL || 'ws://localhost:8080/ws';
const ORCHESTRATOR_API = process.env.ORCHESTRATOR_API || 'http://localhost:8080';
const MAX_EVENT_LOG = 1000;

export const orchestratorEvents = new EventEmitter();
orchestratorEvents.setMaxListeners(200);

const taskStore = new Map<string, TaskInfo>();
const eventLog: EventLogEntry[] = [];
let eventIdCounter = 0;
let wsConnected = false;

// ── Event handling ────────────────────────────────────────────────────────────

function handleEvent(event: OrchestratorEvent): void {
    const now = new Date().toISOString();

    // Update task store
    if ('TaskCreated' in event) {
        const { task_id, name, profile } = event.TaskCreated;
        taskStore.set(task_id, {
            id: task_id,
            name,
            profile,
            state: 'Running',
            created_at: now,
            input_tokens: 0,
            output_tokens: 0,
            cost_usd: 0,
            turns: 0,
            last_activity: now,
        });
    } else if ('TaskStateChanged' in event) {
        const { task_id, new_state } = event.TaskStateChanged;
        const task = taskStore.get(task_id);
        if (task) {
            taskStore.set(task_id, { ...task, state: new_state, last_activity: now });
        }
    } else if ('ConversationRenamed' in event) {
        const { task_id, title } = event.ConversationRenamed;
        const task = taskStore.get(task_id);
        if (task) {
            taskStore.set(task_id, { ...task, name: title, last_activity: now });
        }
    } else if ('TurnComplete' in event) {
        const { task_id, usage } = event.TurnComplete;
        const task = taskStore.get(task_id);
        if (task) {
            taskStore.set(task_id, {
                ...task,
                input_tokens: task.input_tokens + usage.input_tokens,
                output_tokens: task.output_tokens + usage.output_tokens,
                cost_usd: task.cost_usd + usage.total_cost_usd,
                turns: task.turns + usage.turns,
                last_activity: now,
            });
        }
    } else if ('TextOutput' in event) {
        const task = taskStore.get(event.TextOutput.task_id);
        if (task) taskStore.set(event.TextOutput.task_id, { ...task, last_activity: now });
    } else if ('ToolStarted' in event) {
        const task = taskStore.get(event.ToolStarted.task_id);
        if (task) taskStore.set(event.ToolStarted.task_id, { ...task, last_activity: now });
    }

    // Append to event log
    const entry: EventLogEntry = { id: ++eventIdCounter, event, timestamp: now };
    eventLog.push(entry);
    if (eventLog.length > MAX_EVENT_LOG) {
        eventLog.splice(0, eventLog.length - MAX_EVENT_LOG);
    }

    // Emit for SSE subscribers
    orchestratorEvents.emit('event', event);
}

// ── WebSocket client ──────────────────────────────────────────────────────────

let ws: WebSocket | null = null;
let reconnectTimer: ReturnType<typeof setTimeout> | undefined;
let backoff = 1000;

function connect(): void {
    try {
        ws = new WebSocket(ORCHESTRATOR_WS_URL);
    } catch (e) {
        scheduleReconnect();
        return;
    }

    ws.on('open', () => {
        console.log(`[orchestrator] WS connected to ${ORCHESTRATOR_WS_URL}`);
        wsConnected = true;
        backoff = 1000;
        orchestratorEvents.emit('connected', true);
        // Seed task store from the REST API now that the orchestrator is reachable.
        void seedTasksFromApi();
    });

    ws.on('close', () => {
        console.log('[orchestrator] WS disconnected');
        wsConnected = false;
        orchestratorEvents.emit('connected', false);
        scheduleReconnect();
    });

    ws.on('error', (err) => {
        console.error('[orchestrator] WS error:', err.message);
        ws?.terminate();
    });

    ws.on('message', (data: WebSocket.Data) => {
        let event: OrchestratorEvent;
        try {
            event = JSON.parse(data.toString()) as OrchestratorEvent;
        } catch (e) {
            console.error('[orchestrator] Failed to parse WS message:', e);
            return;
        }
        handleEvent(event);
    });
}

function scheduleReconnect(): void {
    if (reconnectTimer !== undefined) return;
    reconnectTimer = setTimeout(() => {
        reconnectTimer = undefined;
        connect();
    }, backoff);
    backoff = Math.min(backoff * 2, 30_000);
}

// Start WS connection
connect();

// Seed initial task state from the orchestrator REST API so tasks created
// before this server started (or before the WS connected) are visible immediately.
async function seedTasksFromApi(): Promise<void> {
    try {
        const data = await callApi('GET', '/api/tasks') as unknown;
        const tasks: TaskInfo[] = Array.isArray(data)
            ? (data as TaskInfo[])
            : ((data as { tasks?: TaskInfo[] }).tasks ?? []);
        const now = new Date().toISOString();
        let seeded = 0;
        for (const task of tasks) {
            if (!taskStore.has(task.id)) {
                taskStore.set(task.id, { ...task, last_activity: task.last_activity || now });
                seeded++;
            }
        }
        if (seeded > 0) console.log(`[orchestrator] Seeded ${seeded} tasks from API`);
    } catch {
        // Orchestrator API not yet available — tasks will arrive via WS events
    }
}
void seedTasksFromApi();

// ── Exports ───────────────────────────────────────────────────────────────────

export function getTasks(): TaskInfo[] {
    return Array.from(taskStore.values()).sort(
        (a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime()
    );
}

export function getMetrics(): MetricsSummary {
    const tasks = Array.from(taskStore.values());
    return {
        total_tasks: tasks.length,
        running_tasks: tasks.filter(t => t.state === 'Running').length,
        hibernated_tasks: tasks.filter(t => t.state === 'Hibernated').length,
        dead_tasks: tasks.filter(t => t.state === 'Dead').length,
        total_cost_usd: tasks.reduce((s, t) => s + t.cost_usd, 0),
        total_input_tokens: tasks.reduce((s, t) => s + t.input_tokens, 0),
        total_output_tokens: tasks.reduce((s, t) => s + t.output_tokens, 0),
        connected: wsConnected,
    };
}

export function getEventLog(): EventLogEntry[] {
    return [...eventLog];
}

export async function callApi(method: string, path: string, body?: unknown): Promise<unknown> {
    const url = `${ORCHESTRATOR_API}${path}`;
    const init: RequestInit = {
        method,
        headers: { 'Content-Type': 'application/json' },
    };
    if (body !== undefined) {
        init.body = JSON.stringify(body);
    }
    const res = await fetch(url, init);
    if (!res.ok) {
        const text = await res.text().catch(() => '');
        throw new Error(`API ${method} ${path} failed (${res.status}): ${text}`);
    }
    const text = await res.text();
    if (!text) return {};
    return JSON.parse(text);
}
