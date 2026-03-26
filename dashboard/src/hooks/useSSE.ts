import { useEffect } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import type { OrchestratorEvent } from '../types';

export function useSSE() {
    const queryClient = useQueryClient();

    useEffect(() => {
        let ws: WebSocket | null = null;
        let reconnectTimer: ReturnType<typeof setTimeout> | undefined;
        let backoff = 1000;

        function connect() {
            const url = `${location.protocol === 'https:' ? 'wss' : 'ws'}://${location.host}/ws`;
            ws = new WebSocket(url);

            ws.onopen = () => {
                backoff = 1000;
                queryClient.setQueryData(['ws_connected'], true);
            };

            ws.onclose = () => {
                queryClient.setQueryData(['ws_connected'], false);
                reconnectTimer = setTimeout(() => {
                    reconnectTimer = undefined;
                    connect();
                }, backoff);
                backoff = Math.min(backoff * 2, 30_000);
            };

            ws.onerror = () => ws?.close();

            ws.onmessage = (e: MessageEvent<string>) => {
                let event: OrchestratorEvent;
                try {
                    event = JSON.parse(e.data) as OrchestratorEvent;
                } catch (err) {
                    console.error('[WS] Parse error:', err);
                    return;
                }
                handleEvent(event);
            };
        }

        function appendHistory(taskId: string, event: OrchestratorEvent) {
            queryClient.setQueryData<OrchestratorEvent[]>(['history', taskId], (old) => [
                ...(old ?? []),
                event,
            ]);
        }

        // tRPC caches tasks.list at this key prefix.
        const tasksQueryKey = [['tasks', 'list']];

        function invalidateTasks() {
            void queryClient.invalidateQueries({ queryKey: tasksQueryKey });
        }

        function handleEvent(event: OrchestratorEvent) {
            if ('TaskCreated' in event) {
                invalidateTasks();
                return;
            }

            if ('TaskStateChanged' in event) {
                invalidateTasks();
                return;
            }

            if ('TurnComplete' in event) {
                const { task_id } = event.TurnComplete;
                invalidateTasks();
                appendHistory(task_id, event);
                return;
            }

            // Route display events to per-task history
            if ('TextOutput' in event) { appendHistory(event.TextOutput.task_id, event); return; }
            if ('ToolStarted' in event) { appendHistory(event.ToolStarted.task_id, event); return; }
            if ('ToolCompleted' in event) { appendHistory(event.ToolCompleted.task_id, event); return; }
            if ('Thinking' in event) { appendHistory(event.Thinking.task_id, event); return; }
            if ('FileOutput' in event) { appendHistory(event.FileOutput.task_id, event); return; }
            if ('CommandResponse' in event && event.CommandResponse.task_id) {
                appendHistory(event.CommandResponse.task_id, event);
                return;
            }
            if ('Error' in event && event.Error.task_id) {
                appendHistory(event.Error.task_id, event);
            }
        }

        connect();

        return () => {
            if (reconnectTimer !== undefined) clearTimeout(reconnectTimer);
            ws?.close();
        };
    }, [queryClient]);
}
