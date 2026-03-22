// Production build — outputs to dist/ (copied to ./static in Docker)
import * as fs from 'node:fs';
import * as child_process from 'node:child_process';
import esbuild from 'esbuild';

fs.mkdirSync('dist', { recursive: true });

// Build JS/TS with esbuild (CSS imports are no-ops; CSS is handled by Tailwind CLI)
await esbuild.build({
    entryPoints: ['src/main.tsx'],
    bundle: true,
    outdir: 'dist',
    format: 'esm',
    platform: 'browser',
    minify: true,
    sourcemap: true,
    define: { 'process.env.NODE_ENV': '"production"' },
    loader: { '.css': 'empty', '.svg': 'dataurl' },
    target: ['es2022'],
});

// Build CSS with Tailwind CLI
child_process.execSync('node_modules/.bin/tailwindcss -i src/index.css -o dist/main.css --minify', {
    stdio: 'inherit',
});

// Copy and update index.html
let html = fs.readFileSync('index.html', 'utf8');
html = html.replace(
    '<script type="module" src="/src/main.tsx"></script>',
    '<link rel="stylesheet" href="/main.css"><script type="module" src="/main.js"></script>',
);
fs.writeFileSync('dist/index.html', html);

console.log('Build complete → dist/');
