import type { SessionInfo, SlashCommand, ClaudeEvent } from '../types';

const BASE = '/api';

async function apiFetch<T>(path: string, init?: RequestInit): Promise<T> {
    const res = await fetch(`${BASE}${path}`, init);
    if (!res.ok) {
        const body = await res.text().catch(() => '');
        throw new Error(`API ${path} failed (${res.status}): ${body}`);
    }
    return res.json() as Promise<T>;
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

export interface StatusData {
    connected: boolean;
    hostname: string | null;
    commands: SlashCommand[];
}

export function fetchStatus(): Promise<StatusData> {
    return apiFetch('/status');
}

// ---------------------------------------------------------------------------
// Sessions
// ---------------------------------------------------------------------------

export function fetchSessions(): Promise<{ sessions: SessionInfo[] }> {
    return apiFetch('/sessions');
}

export function createSession(params: {
    name?: string;
    initial_prompt?: string;
}): Promise<{ session: SessionInfo }> {
    return apiFetch('/sessions', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(params),
    });
}

// ---------------------------------------------------------------------------
// Session actions
// ---------------------------------------------------------------------------

export function fetchHistory(
    sessionId: string,
): Promise<{ session_id: string; events: ClaudeEvent[] }> {
    return apiFetch(`/sessions/${sessionId}/history`);
}

export function sendInput(sessionId: string, text: string): Promise<{ ok: boolean }> {
    return apiFetch(`/sessions/${sessionId}/input`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ text }),
    });
}

export function killSession(sessionId: string): Promise<{ ok: boolean }> {
    return apiFetch(`/sessions/${sessionId}`, { method: 'DELETE' });
}
