import { test, expect } from '@playwright/test';

test('health endpoint returns ok', async ({ request }) => {
    const response = await request.get('/api/health');
    expect(response.status()).toBe(200);
    const body = await response.json() as { ok: boolean; auth_required: boolean };
    expect(body.ok).toBe(true);
    expect(typeof body.auth_required).toBe('boolean');
});

test('tRPC metrics endpoint responds', async ({ request }) => {
    const response = await request.get('/trpc/metrics.summary');
    // Should return 200 even without orchestrator connection (metrics may be empty)
    expect([200, 401]).toContain(response.status());
});
