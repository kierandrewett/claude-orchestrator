import { createTRPCReact } from '@trpc/react-query';
import { httpBatchLink } from '@trpc/client';
import type { AppRouter } from '../../server/src/routers/index';

export const trpc = createTRPCReact<AppRouter>();

export function getAuthHeaders(): Record<string, string> {
    const token = localStorage.getItem('dashboard_token');
    return token ? { Authorization: `Bearer ${token}` } : {};
}

export function createTrpcClient() {
    return trpc.createClient({
        links: [
            httpBatchLink({
                url: '/trpc',
                headers: getAuthHeaders,
            }),
        ],
    });
}
