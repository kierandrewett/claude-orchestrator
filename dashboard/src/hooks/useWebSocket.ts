import { useEffect } from 'react';
import { wsClient } from '../lib/ws';
import { useSessionsStore } from '../store/sessions';

export function useWebSocket() {
    const handleMessage = useSessionsStore((s) => s.handleMessage);
    const setWsConnected = useSessionsStore((s) => s.setWsConnected);

    useEffect(() => {
        wsClient.connect();

        const unsubMsg = wsClient.subscribe(handleMessage);
        const unsubConn = wsClient.subscribeConnection(setWsConnected);

        // Sync initial connection state
        setWsConnected(wsClient.isConnected);

        return () => {
            unsubMsg();
            unsubConn();
        };
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, []);
}
