import { useEffect } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import type { S2DMessage, SessionInfo, ClaudeEvent } from '../types';

export function useSSE() {
    const queryClient = useQueryClient();

    useEffect(() => {
        let es: EventSource | null = null;
        let reconnectTimer: ReturnType<typeof setTimeout> | undefined;
        let backoff = 1000;

        function connect() {
            es = new EventSource('/api/events');

            es.onopen = () => {
                console.log('[SSE] Connected');
                backoff = 1000;
            };

            es.onmessage = (event: MessageEvent<string>) => {
                let msg: S2DMessage;
                try {
                    msg = JSON.parse(event.data) as S2DMessage;
                } catch (e) {
                    console.error('[SSE] Parse error:', e);
                    return;
                }
                handleMessage(msg);
            };

            es.onerror = () => {
                console.warn('[SSE] Error — reconnecting in', backoff, 'ms');
                es?.close();
                es = null;
                reconnectTimer = setTimeout(() => {
                    reconnectTimer = undefined;
                    connect();
                }, backoff);
                backoff = Math.min(backoff * 2, 30_000);
            };
        }

        function handleMessage(msg: S2DMessage) {
            switch (msg.type) {
                case 'session_list':
                    queryClient.setQueryData<{ sessions: SessionInfo[] }>(['sessions'], {
                        sessions: msg.sessions,
                    });
                    break;

                case 'session_created':
                    queryClient.setQueryData<{ sessions: SessionInfo[] }>(['sessions'], (old) => ({
                        sessions: [
                            msg.session,
                            ...(old?.sessions ?? []).filter((s) => s.id !== msg.session.id),
                        ],
                    }));
                    break;

                case 'session_updated':
                    queryClient.setQueryData<{ sessions: SessionInfo[] }>(['sessions'], (old) => ({
                        sessions: (old?.sessions ?? []).map((s) =>
                            s.id === msg.session.id ? msg.session : s,
                        ),
                    }));
                    break;

                case 'session_event':
                    queryClient.setQueryData<{ session_id: string; events: ClaudeEvent[] }>(
                        ['history', msg.session_id],
                        (old) => ({
                            session_id: msg.session_id,
                            events: old ? [...old.events, msg.event] : [msg.event],
                        }),
                    );
                    break;

                case 'session_history':
                    queryClient.setQueryData(['history', msg.session_id], {
                        session_id: msg.session_id,
                        events: msg.events,
                    });
                    break;

                case 'session_ended':
                    queryClient.setQueryData<{ sessions: SessionInfo[] }>(['sessions'], (old) => ({
                        sessions: (old?.sessions ?? []).map((s) =>
                            s.id === msg.session_id ? { ...s, stats: msg.stats } : s,
                        ),
                    }));
                    break;

                case 'client_status':
                    queryClient.setQueryData(
                        ['status'],
                        (
                            old:
                                | {
                                      connected: boolean;
                                      hostname: string | null;
                                      commands: unknown[];
                                  }
                                | undefined,
                        ) => ({
                            ...(old ?? { commands: [] }),
                            connected: msg.connected,
                            hostname: msg.hostname,
                        }),
                    );
                    break;

                case 'command_list':
                    queryClient.setQueryData(
                        ['status'],
                        (old: { connected: boolean; hostname: string | null } | undefined) => ({
                            ...(old ?? { connected: false, hostname: null }),
                            commands: msg.commands,
                        }),
                    );
                    break;

                case 'error':
                    console.error('[SSE] Server error:', msg.message);
                    break;
            }
        }

        connect();

        return () => {
            if (reconnectTimer !== undefined) clearTimeout(reconnectTimer);
            es?.close();
        };
    }, [queryClient]);
}
