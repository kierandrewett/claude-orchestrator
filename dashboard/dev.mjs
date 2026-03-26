// Dev server — runs Vite + Fastify together
// Uses esbuild watch to rebuild the server, then restarts node on each successful build.
import { spawn } from 'node:child_process';
import { context } from 'esbuild';
import { mkdirSync } from 'node:fs';

const env = { ...process.env };
const HOME = env.HOME || '/root';

const SERVER_ENV = {
    ...env,
    PORT: '3001',
    ORCHESTRATOR_API: env.ORCHESTRATOR_API || 'http://localhost:8080',
    ORCHESTRATOR_WS_URL: env.ORCHESTRATOR_WS_URL || 'ws://localhost:8080/ws',
    CONFIG_PATH: env.CONFIG_PATH || '../config/orchestrator.toml',
    STATE_DIR: env.STATE_DIR || `${HOME}/.local/share/claude-orchestrator`,
};

mkdirSync('dist-server', { recursive: true });

let serverProcess = null;

function restartServer() {
    if (serverProcess) {
        serverProcess.kill();
        serverProcess = null;
    }
    serverProcess = spawn('node', ['dist-server/index.cjs'], {
        stdio: 'inherit',
        env: SERVER_ENV,
    });
    serverProcess.on('exit', (code) => {
        if (code !== 0 && code !== null) console.error(`[server] exited with code ${code}`);
    });
}

// esbuild watch context - rebuilds on change, restarts server after each successful build
const ctx = await context({
    entryPoints: ['server/src/index.ts'],
    bundle: true,
    platform: 'node',
    target: 'node20',
    outfile: 'dist-server/index.cjs',
    format: 'cjs',
    external: [
        'fastify',
        '@fastify/cors',
        '@fastify/static',
        '@fastify/websocket',
        'ws',
        '@iarna/toml',
        '@trpc/server',
    ],
    plugins: [{
        name: 'restart-on-build',
        setup(build) {
            build.onEnd((result) => {
                if (result.errors.length === 0) {
                    console.log('[server] rebuilt, restarting...');
                    restartServer();
                } else {
                    console.error('[server] build errors, not restarting');
                }
            });
        },
    }],
});

await ctx.watch();
console.log('[server] esbuild watching server/src/...');

// Start Vite dev server
const vite = spawn('node_modules/.bin/vite', [], { stdio: 'inherit' });

function shutdown() {
    if (serverProcess) serverProcess.kill();
    vite.kill();
    ctx.dispose();
    process.exit(0);
}

process.on('SIGINT', shutdown);
process.on('SIGTERM', shutdown);

vite.on('exit', (code) => {
    if (code !== 0 && code !== null) console.error(`[vite] exited with code ${code}`);
});
