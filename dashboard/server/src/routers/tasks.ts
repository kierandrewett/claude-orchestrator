import { z } from 'zod';
import { router, publicProcedure } from '../trpc.js';
import { getTasks, callApi } from '../lib/orchestrator.js';

export const tasksRouter = router({
    list: publicProcedure.query(() => getTasks()),

    profiles: publicProcedure.query(async () => {
        try {
            const data = await callApi('GET', '/api/profiles');
            return data as { profiles: string[] };
        } catch {
            return { profiles: [] };
        }
    }),

    create: publicProcedure
        .input(z.object({
            profile: z.string().optional(),
            prompt: z.string().optional(),
        }))
        .mutation(async ({ input }) => {
            return callApi('POST', '/api/tasks', input);
        }),

    stop: publicProcedure
        .input(z.string())
        .mutation(async ({ input: id }) => {
            return callApi('DELETE', `/api/tasks/${id}`);
        }),

    hibernate: publicProcedure
        .input(z.string())
        .mutation(async ({ input: id }) => {
            return callApi('POST', `/api/tasks/${id}/hibernate`);
        }),

    message: publicProcedure
        .input(z.object({ id: z.string(), text: z.string() }))
        .mutation(async ({ input }) => {
            return callApi('POST', `/api/tasks/${input.id}/message`, { text: input.text });
        }),

    wake: publicProcedure
        .input(z.string())
        .mutation(async ({ input: id }) => {
            return callApi('POST', `/api/tasks/${id}/wake`);
        }),

    history: publicProcedure
        .input(z.string())
        .query(async ({ input: id }) => {
            try {
                return callApi('GET', `/api/tasks/${id}/history`);
            } catch {
                return { events: [] };
            }
        }),
});
