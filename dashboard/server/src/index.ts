import Fastify from 'fastify';
import cors from '@fastify/cors';
import staticFiles from '@fastify/static';
import { fastifyTRPCPlugin } from '@trpc/server/adapters/fastify';
import * as path from 'path';
import * as fs from 'fs';
import { fileURLToPath } from 'url';
import { appRouter } from './routers/index.js';
import { createContext } from './context.js';
import { orchestratorEvents, getEventLog, type OrchestratorEvent } from './lib/orchestrator.js';

// ── Config from env ───────────────────────────────────────────────────────────

const PORT = parseInt(process.env.PORT ?? '3001', 10);
const DASHBOARD_TOKEN = process.env.DASHBOARD_TOKEN ?? '';

// ── Dir resolution ────────────────────────────────────────────────────────────

function getDirname(): string {
    try {
        // ESM
        return path.dirname(fileURLToPath(import.meta.url));
    } catch {
        // CJS fallback
        return __dirname;
    }
}

async function main() {
    // ── Fastify instance ──────────────────────────────────────────────────────────

    const server = Fastify({
        logger: {
            level: process.env.LOG_LEVEL ?? 'info',
        },
        routerOptions: { maxParamLength: 5000 },
    });

    // CORS
    await server.register(cors, {
        origin: true,
        credentials: true,
    });

    // ── Auth helper ───────────────────────────────────────────────────────────────

    function checkAuth(authHeader: string | undefined): boolean {
        if (!DASHBOARD_TOKEN) return true;
        return authHeader === `Bearer ${DASHBOARD_TOKEN}`;
    }

    // ── Auth hook for tRPC and /api routes ────────────────────────────────────────

    server.addHook('onRequest', async (request, reply) => {
        const url = request.url;
        if (!url.startsWith('/trpc') && !url.startsWith('/api')) return;
        if (url === '/api/health') return;
        if (!checkAuth(request.headers.authorization)) {
            return reply.code(401).send({ error: 'Unauthorized' });
        }
    });

    // ── tRPC ──────────────────────────────────────────────────────────────────────

    await server.register(fastifyTRPCPlugin, {
        prefix: '/trpc',
        trpcOptions: { router: appRouter, createContext },
    });

    // ── Health ────────────────────────────────────────────────────────────────────

    server.get('/api/health', async () => {
        return { ok: true, auth_required: Boolean(DASHBOARD_TOKEN) };
    });

    // ── SSE events stream ─────────────────────────────────────────────────────────

    server.get('/api/events/stream', async (request, reply) => {
        reply.raw.writeHead(200, {
            'Content-Type': 'text/event-stream',
            'Cache-Control': 'no-cache',
            'Connection': 'keep-alive',
            'X-Accel-Buffering': 'no',
        });

        // Flush recent events
        const recent = getEventLog().slice(-50);
        for (const entry of recent) {
            reply.raw.write(`data: ${JSON.stringify(entry.event)}\n\n`);
        }

        const handler = (event: OrchestratorEvent) => {
            reply.raw.write(`data: ${JSON.stringify(event)}\n\n`);
        };

        orchestratorEvents.on('event', handler);

        // Keep alive ping
        const keepAlive = setInterval(() => {
            reply.raw.write(': keepalive\n\n');
        }, 15_000);

        await new Promise<void>((resolve) => {
            request.raw.on('close', () => {
                orchestratorEvents.off('event', handler);
                clearInterval(keepAlive);
                resolve();
            });
        });
    });

    // ── Static file serving (production) ─────────────────────────────────────────

    const distDir = path.join(getDirname(), '../../dist');
    if (fs.existsSync(distDir)) {
        await server.register(staticFiles, {
            root: distDir,
            prefix: '/',
            index: false,
        });

        // SPA fallback
        server.setNotFoundHandler(async (request, reply) => {
            if (!request.url.startsWith('/api') && !request.url.startsWith('/trpc')) {
                const indexPath = path.join(distDir, 'index.html');
                if (fs.existsSync(indexPath)) {
                    reply.type('text/html');
                    return reply.send(fs.readFileSync(indexPath));
                }
            }
            return reply.code(404).send({ error: 'Not found' });
        });
    }

    // ── Start ─────────────────────────────────────────────────────────────────────

    try {
        await server.listen({ port: PORT, host: '0.0.0.0' });
        console.log(`Dashboard server listening on http://0.0.0.0:${PORT}`);
    } catch (err) {
        server.log.error(err);
        process.exit(1);
    }
}

main().catch(console.error);
