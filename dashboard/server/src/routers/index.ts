import { router } from '../trpc.js';
import { tasksRouter } from './tasks.js';
import { configRouter } from './config.js';
import { mcpRouter } from './mcp.js';
import { schedulerRouter } from './scheduler.js';
import { metricsRouter } from './metrics.js';

export const appRouter = router({
    tasks: tasksRouter,
    config: configRouter,
    mcp: mcpRouter,
    scheduler: schedulerRouter,
    metrics: metricsRouter,
});

export type AppRouter = typeof appRouter;
