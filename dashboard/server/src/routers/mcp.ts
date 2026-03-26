import { z } from 'zod';
import { router, publicProcedure } from '../trpc.js';
import { listServers, addServer, removeServer, toggleServer } from '../lib/mcp-registry.js';

const STATE_DIR = process.env.STATE_DIR || `${process.env.HOME}/.local/share/claude-orchestrator`;

const McpServerEntryInput = z.object({
    name: z.string().min(1),
    command: z.string().optional(),
    args: z.array(z.string()).optional(),
    env: z.record(z.string()).optional(),
    url: z.string().nullable().optional(),
});

export const mcpRouter = router({
    list: publicProcedure.query(async () => {
        return listServers(STATE_DIR);
    }),

    add: publicProcedure
        .input(McpServerEntryInput)
        .mutation(async ({ input }) => {
            await addServer(STATE_DIR, input);
            return { ok: true };
        }),

    remove: publicProcedure
        .input(z.string())
        .mutation(async ({ input: name }) => {
            await removeServer(STATE_DIR, name);
            return { ok: true };
        }),

    toggle: publicProcedure
        .input(z.object({ name: z.string(), enabled: z.boolean() }))
        .mutation(async ({ input }) => {
            await toggleServer(STATE_DIR, input.name, input.enabled);
            return { ok: true };
        }),
});
