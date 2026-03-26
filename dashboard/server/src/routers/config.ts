import { z } from 'zod';
import { router, publicProcedure } from '../trpc.js';
import { readConfig, updateConfigSection } from '../lib/toml-manager.js';

const CONFIG_PATH = process.env.CONFIG_PATH || '../config/orchestrator.toml';

export const configRouter = router({
    get: publicProcedure.query(async () => {
        return readConfig(CONFIG_PATH);
    }),

    updateSection: publicProcedure
        .input(z.object({
            section: z.string(),
            values: z.record(z.unknown()),
        }))
        .mutation(async ({ input }) => {
            await updateConfigSection(CONFIG_PATH, input.section, input.values);
            return { ok: true };
        }),

    reload: publicProcedure.query(async () => {
        const config = await readConfig(CONFIG_PATH);
        return { ok: true, config };
    }),
});
