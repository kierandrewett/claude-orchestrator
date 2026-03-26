import { z } from 'zod';
import { router, publicProcedure } from '../trpc.js';
import { listServers, addServer, removeServer, toggleServer } from '../lib/mcp-registry.js';
import { getTasks, callApi } from '../lib/orchestrator.js';

const STATE_DIR = process.env.STATE_DIR || `${process.env.HOME}/.local/share/claude-orchestrator`;

const McpServerEntryInput = z.object({
    name: z.string().min(1),
    command: z.string().optional(),
    args: z.array(z.string()).optional(),
    env: z.record(z.string()).optional(),
    url: z.string().nullable().optional(),
});

/** Wake all hibernated tasks so they pick up the new MCP config on next session start. */
async function wakeHibernatedTasks(): Promise<void> {
    const tasks = getTasks().filter(t => t.state === 'Hibernated');
    await Promise.allSettled(
        tasks.map(t => callApi('POST', `/api/tasks/${t.id}/wake`))
    );
    if (tasks.length > 0) {
        console.log(`[mcp] woke ${tasks.length} hibernated task(s) after MCP config change`);
    }
}

export const mcpRouter = router({
    list: publicProcedure.query(async () => {
        return listServers(STATE_DIR);
    }),

    add: publicProcedure
        .input(McpServerEntryInput)
        .mutation(async ({ input }) => {
            await addServer(STATE_DIR, input);
            void wakeHibernatedTasks();
            return { ok: true };
        }),

    remove: publicProcedure
        .input(z.string())
        .mutation(async ({ input: name }) => {
            await removeServer(STATE_DIR, name);
            void wakeHibernatedTasks();
            return { ok: true };
        }),

    toggle: publicProcedure
        .input(z.object({ name: z.string(), enabled: z.boolean() }))
        .mutation(async ({ input }) => {
            await toggleServer(STATE_DIR, input.name, input.enabled);
            void wakeHibernatedTasks();
            return { ok: true };
        }),
});
