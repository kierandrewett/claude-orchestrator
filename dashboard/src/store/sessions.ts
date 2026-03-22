import { create } from 'zustand';
import { immer } from 'zustand/middleware/immer';
import type { SessionInfo, SessionStats, ClaudeEvent, S2DMessage, SlashCommand } from '../types';
import { wsClient } from '../lib/ws';

export interface ClientStatus {
    connected: boolean;
    hostname: string | null;
}

interface SessionsStore {
    sessions: Record<string, SessionInfo>;
    events: Record<string, ClaudeEvent[]>;
    wsConnected: boolean;
    clientStatus: ClientStatus;
    commands: SlashCommand[];

    handleMessage: (msg: S2DMessage) => void;
    createSession: (params: { name?: string; initial_prompt?: string }) => void;
    sendInput: (session_id: string, text: string) => void;
    killSession: (session_id: string) => void;
    requestHistory: (session_id: string) => void;
    setWsConnected: (v: boolean) => void;
}

export const useSessionsStore = create<SessionsStore>()(
    immer((set) => ({
        sessions: {},
        events: {},
        wsConnected: false,
        clientStatus: { connected: false, hostname: null },
        commands: [],

        handleMessage: (msg: S2DMessage) =>
            set((state) => {
                switch (msg.type) {
                    case 'session_list':
                        state.sessions = Object.fromEntries(
                            msg.sessions.map((s: SessionInfo) => [s.id, s]),
                        );
                        break;
                    case 'session_created':
                    case 'session_updated':
                        state.sessions[msg.session.id] = msg.session;
                        break;
                    case 'session_event':
                        if (!state.events[msg.session_id]) {
                            state.events[msg.session_id] = [];
                        }
                        state.events[msg.session_id].push(msg.event);
                        break;
                    case 'session_history':
                        state.events[msg.session_id] = msg.events;
                        break;
                    case 'session_ended':
                        if (state.sessions[msg.session_id]) {
                            (state.sessions[msg.session_id] as SessionInfo).stats =
                                msg.stats as SessionStats;
                        }
                        break;
                    case 'client_status':
                        state.clientStatus = {
                            connected: msg.connected,
                            hostname: msg.hostname,
                        };
                        break;
                    case 'command_list':
                        state.commands = msg.commands;
                        break;
                    case 'error':
                        console.error('[Store] Server error:', msg.message);
                        break;
                }
            }),

        createSession: (params) => wsClient.send({ type: 'create_session', ...params }),

        sendInput: (session_id, text) => wsClient.send({ type: 'send_input', session_id, text }),

        killSession: (session_id) => wsClient.send({ type: 'kill_session', session_id }),

        requestHistory: (session_id) => wsClient.send({ type: 'get_history', session_id }),

        setWsConnected: (v) =>
            set((state) => {
                state.wsConnected = v;
            }),
    })),
);

export const useCommands = () => useSessionsStore((s) => s.commands);
