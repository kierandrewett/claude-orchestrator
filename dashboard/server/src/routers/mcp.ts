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
    transport: z.string().optional(),
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

interface SessionToolsResponse {
    tools: string[];
    has_running_session: boolean;
}

async function getSessionTools(): Promise<SessionToolsResponse> {
    try {
        return await callApi('GET', '/api/mcp/session-tools') as SessionToolsResponse;
    } catch {
        return { tools: [], has_running_session: false };
    }
}

export const mcpRouter = router({
    list: publicProcedure.query(async () => {
        const [servers, { tools, has_running_session }] = await Promise.all([
            listServers(STATE_DIR),
            getSessionTools(),
        ]);

        // Derive per-server connection status from session tool names.
        // Claude Code names MCP tools as `mcp__<servername>__<toolname>`.
        return servers.map(server => {
            let connected: boolean | null = null;
            if (has_running_session) {
                const prefix = `mcp__${server.name}__`;
                connected = tools.some(t => t.startsWith(prefix));
            }
            return { ...server, connected };
        });
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
