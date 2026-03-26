// Production build
import { build } from 'esbuild';
import * as fs from 'node:fs';
import * as child_process from 'node:child_process';

fs.mkdirSync('dist', { recursive: true });
fs.mkdirSync('dist-server', { recursive: true });

// ── React frontend (Vite) ─────────────────────────────────────────────────────

console.log('Building React frontend with Vite...');
child_process.execSync('npx vite build', { stdio: 'inherit' });
console.log('Frontend build complete → dist/');

// ── Fastify server (esbuild) ──────────────────────────────────────────────────

console.log('Building Fastify server...');
await build({
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
});
console.log('Server build complete → dist-server/index.cjs');

console.log('Build complete!');
