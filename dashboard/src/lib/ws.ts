import type { S2DMessage, D2SMessage } from '../types';

type MessageHandler = (msg: S2DMessage) => void;
type ConnectionHandler = (connected: boolean) => void;

class WsClient {
    private ws: WebSocket | null = null;
    private messageHandlers: Set<MessageHandler> = new Set();
    private connectionHandlers: Set<ConnectionHandler> = new Set();
    private reconnectTimer?: ReturnType<typeof setTimeout>;
    private backoff = 1000;
    private readonly maxBackoff = 30_000;
    private url: string;
    private shouldReconnect = true;
    private connecting = false;

    constructor() {
        const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
        this.url = `${proto}//${window.location.host}/ws/dashboard`;
    }

    connect(): void {
        if (this.connecting || (this.ws && this.ws.readyState === WebSocket.OPEN)) return;
        this.connecting = true;
        this.shouldReconnect = true;

        try {
            this.ws = new WebSocket(this.url);
        } catch (err) {
            console.error('[WsClient] Failed to create WebSocket:', err);
            this.connecting = false;
            this.scheduleReconnect();
            return;
        }

        this.ws.onopen = () => {
            console.log('[WsClient] Connected');
            this.connecting = false;
            this.backoff = 1000;
            this.notifyConnection(true);
        };

        this.ws.onmessage = (ev) => {
            this.onMessage(ev.data as string);
        };

        this.ws.onclose = (ev) => {
            console.log('[WsClient] Closed', ev.code, ev.reason);
            this.connecting = false;
            this.ws = null;
            this.notifyConnection(false);
            if (this.shouldReconnect) {
                this.scheduleReconnect();
            }
        };

        this.ws.onerror = (err) => {
            console.error('[WsClient] Error', err);
            this.connecting = false;
            // onclose will fire after onerror, which handles reconnect
        };
    }

    disconnect(): void {
        this.shouldReconnect = false;
        if (this.reconnectTimer !== undefined) {
            clearTimeout(this.reconnectTimer);
            this.reconnectTimer = undefined;
        }
        if (this.ws) {
            this.ws.close();
            this.ws = null;
        }
    }

    send(msg: D2SMessage): void {
        if (this.ws && this.ws.readyState === WebSocket.OPEN) {
            this.ws.send(JSON.stringify(msg));
        } else {
            console.warn('[WsClient] Cannot send — not connected:', msg.type);
        }
    }

    subscribe(handler: MessageHandler): () => void {
        this.messageHandlers.add(handler);
        return () => {
            this.messageHandlers.delete(handler);
        };
    }

    subscribeConnection(handler: ConnectionHandler): () => void {
        this.connectionHandlers.add(handler);
        return () => {
            this.connectionHandlers.delete(handler);
        };
    }

    get isConnected(): boolean {
        return this.ws?.readyState === WebSocket.OPEN;
    }

    private onMessage(data: string): void {
        let msg: S2DMessage;
        try {
            msg = JSON.parse(data) as S2DMessage;
        } catch (err) {
            console.error('[WsClient] Failed to parse message:', err, data);
            return;
        }
        for (const handler of this.messageHandlers) {
            try {
                handler(msg);
            } catch (err) {
                console.error('[WsClient] Handler error:', err);
            }
        }
    }

    private notifyConnection(connected: boolean): void {
        for (const handler of this.connectionHandlers) {
            try {
                handler(connected);
            } catch (err) {
                console.error('[WsClient] Connection handler error:', err);
            }
        }
    }

    private scheduleReconnect(): void {
        if (this.reconnectTimer !== undefined) return;
        console.log(`[WsClient] Reconnecting in ${this.backoff}ms`);
        this.reconnectTimer = setTimeout(() => {
            this.reconnectTimer = undefined;
            this.connect();
        }, this.backoff);
        this.backoff = Math.min(this.backoff * 2, this.maxBackoff);
    }
}

export const wsClient = new WsClient();
