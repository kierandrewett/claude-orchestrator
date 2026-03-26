import { router, publicProcedure } from '../trpc.js';
import { getMetrics, getEventLog } from '../lib/orchestrator.js';

export const metricsRouter = router({
    summary: publicProcedure.query(() => getMetrics()),

    eventLog: publicProcedure.query(() => getEventLog().slice(-100)),
});
