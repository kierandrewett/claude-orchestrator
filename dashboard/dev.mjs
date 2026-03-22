// Dev server — watches + builds to dist/, proxies /api/* to backend
import * as http from 'node:http';
import * as fs from 'node:fs';
import * as child_process from 'node:child_process';
import esbuild from 'esbuild';

const BACKEND = process.env.BACKEND_URL || 'http://localhost:8080';
const PORT = parseInt(process.env.DEV_PORT || '3001');

fs.mkdirSync('dist', { recursive: true });

// Write index.html to dist/ with updated refs
let html = fs.readFileSync('index.html', 'utf8');
html = html.replace(
    '<script type="module" src="/src/main.tsx"></script>',
    '<link rel="stylesheet" href="/main.css"><script type="module" src="/main.js"></script>',
);
fs.writeFileSync('dist/index.html', html);

// Initial CSS build
child_process.execSync('node_modules/.bin/tailwindcss -i src/index.css -o dist/main.css', { stdio: 'inherit' });

// Watch CSS in the background
const cssWatcher = child_process.spawn(
    'node_modules/.bin/tailwindcss',
    ['-i', 'src/index.css', '-o', 'dist/main.css', '--watch'],
    { stdio: 'inherit' },
);

// JS/TS build context
const ctx = await esbuild.context({
    entryPoints: ['src/main.tsx'],
    bundle: true,
    outdir: 'dist',
    format: 'esm',
    platform: 'browser',
    sourcemap: true,
    define: { 'process.env.NODE_ENV': '"development"' },
    loader: { '.css': 'empty', '.svg': 'dataurl' },
    target: ['es2022'],
});

await ctx.watch();

// esbuild serves static files from dist/
const { port: esbuildPort } = await ctx.serve({ servedir: 'dist', port: 0 });

// Proxy server
const server = http.createServer((req, res) => {
    const url = req.url || '/';
    const isBackend = url.startsWith('/api/') || url.startsWith('/ws/');

    const upstreamUrl = isBackend ? new URL(url, BACKEND) : null;
    const options = isBackend
        ? {
              hostname: upstreamUrl.hostname,
              port: upstreamUrl.port || '80',
              path: upstreamUrl.pathname + (upstreamUrl.search || ''),
              method: req.method,
              headers: { ...req.headers, host: upstreamUrl.host },
          }
        : {
              hostname: 'localhost',
              port: esbuildPort,
              path: url,
              method: req.method,
              headers: req.headers,
          };

    const proxyReq = http.request(options, (proxyRes) => {
        if (!isBackend && proxyRes.statusCode === 404) {
            const idx = fs.readFileSync('dist/index.html');
            res.writeHead(200, { 'Content-Type': 'text/html' });
            res.end(idx);
            return;
        }
        res.writeHead(proxyRes.statusCode ?? 200, proxyRes.headers);
        proxyRes.pipe(res, { end: true });
    });

    proxyReq.on('error', (err) => {
        console.error(`[proxy] ${url}:`, err.message);
        res.writeHead(502);
        res.end('Bad Gateway');
    });

    req.pipe(proxyReq, { end: true });
});

server.listen(PORT, () => {
    console.log(`Dev server:  http://localhost:${PORT}`);
    console.log(`Backend:     ${BACKEND}`);
    console.log('Watching for changes...');
});

process.on('SIGINT', () => {
    cssWatcher.kill();
    process.exit(0);
});
