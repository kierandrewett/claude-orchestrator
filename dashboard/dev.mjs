// Dev server — runs Vite + Fastify together
import { spawn } from 'node:child_process';

const env = { ...process.env };
const HOME = env.HOME || '/root';

// Start the Fastify server
const server = spawn('node_modules/.bin/tsx', ['--watch', 'server/src/index.ts'], {
    stdio: 'inherit',
    env: {
        ...env,
        PORT: '3001',
        ORCHESTRATOR_API: env.ORCHESTRATOR_API || 'http://localhost:8080',
        ORCHESTRATOR_WS_URL: env.ORCHESTRATOR_WS_URL || 'ws://localhost:8080/ws',
        CONFIG_PATH: env.CONFIG_PATH || '../config/orchestrator.toml',
        STATE_DIR: env.STATE_DIR || `${HOME}/.local/share/claude-orchestrator`,
    },
});

// Start Vite dev server
const vite = spawn('node_modules/.bin/vite', [], { stdio: 'inherit' });

process.on('SIGINT', () => {
    server.kill();
    vite.kill();
    process.exit(0);
});

process.on('SIGTERM', () => {
    server.kill();
    vite.kill();
    process.exit(0);
});

server.on('exit', (code) => {
    if (code !== 0 && code !== null) console.error(`[server] exited with code ${code}`);
});

vite.on('exit', (code) => {
    if (code !== 0 && code !== null) console.error(`[vite] exited with code ${code}`);
});
