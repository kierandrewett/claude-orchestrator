import { defineConfig } from '@playwright/test';

export default defineConfig({
    testDir: './e2e',
    use: {
        baseURL: 'http://localhost:3001',
    },
    webServer: {
        command: 'PORT=3001 ORCHESTRATOR_API=http://localhost:8080 npx tsx server/src/index.ts',
        port: 3001,
        reuseExistingServer: true,
        timeout: 10000,
    },
});
