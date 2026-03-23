import type { TaskInfo } from '../types';

const BASE = '/api';

async function apiFetch<T>(path: string, init?: RequestInit): Promise<T> {
    const res = await fetch(`${BASE}${path}`, init);
    if (!res.ok) {
        const body = await res.text().catch(() => '');
        throw new Error(`API ${path} failed (${res.status}): ${body}`);
    }
    return res.json() as Promise<T>;
}

export function fetchTasks(): Promise<{ tasks: TaskInfo[] }> {
    return apiFetch('/tasks');
}

export function createTask(params: {
    profile?: string;
    prompt?: string;
}): Promise<{ status: string }> {
    return apiFetch('/tasks', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(params),
    });
}

export function sendInput(taskId: string, text: string): Promise<{ status: string }> {
    return apiFetch(`/tasks/${taskId}/message`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ text }),
    });
}

export function stopTask(taskId: string): Promise<{ status: string }> {
    return apiFetch(`/tasks/${taskId}`, { method: 'DELETE' });
}

export function hibernateTask(taskId: string): Promise<{ status: string }> {
    return apiFetch(`/tasks/${taskId}/hibernate`, { method: 'POST' });
}
