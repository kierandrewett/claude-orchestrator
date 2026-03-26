import { z } from 'zod';
import { router, publicProcedure } from '../trpc.js';
import { callApi } from '../lib/orchestrator.js';

export interface ScheduledEvent {
    id: string;
    name: string;
    cron: string;
    enabled: boolean;
    mode?: string;
    prompt?: string | null;
    next_run?: string | null;
    last_run?: string | null;
    origin_task_name?: string;
}

export const schedulerRouter = router({
    list: publicProcedure.query(async () => {
        try {
            const data = await callApi('GET', '/api/scheduled-events');
            return data as { events: ScheduledEvent[] };
        } catch {
            return { events: [] };
        }
    }),

    create: publicProcedure
        .input(z.object({
            name: z.string(),
            cron: z.string(),
            enabled: z.boolean().optional().default(true),
            task_profile: z.string().optional(),
            prompt: z.string().optional(),
        }))
        .mutation(async ({ input }) => {
            return callApi('POST', '/api/scheduled-events', input);
        }),

    update: publicProcedure
        .input(z.object({
            id: z.string(),
            name: z.string().optional(),
            cron: z.string().optional(),
            enabled: z.boolean().optional(),
            task_profile: z.string().optional(),
            prompt: z.string().optional(),
        }))
        .mutation(async ({ input: { id, ...rest } }) => {
            return callApi('PUT', `/api/scheduled-events/${id}`, rest);
        }),

    delete: publicProcedure
        .input(z.string())
        .mutation(async ({ input: id }) => {
            return callApi('DELETE', `/api/scheduled-events/${id}`);
        }),

    toggle: publicProcedure
        .input(z.object({ id: z.string(), enabled: z.boolean() }))
        .mutation(async ({ input }) => {
            const endpoint = input.enabled ? 'enable' : 'disable';
            return callApi('POST', `/api/scheduled-events/${input.id}/${endpoint}`);
        }),
});
