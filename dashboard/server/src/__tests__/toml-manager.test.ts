import { describe, it, expect } from 'vitest';
import { readConfig, updateConfigSection } from '../lib/toml-manager.js';
import { writeFileSync, unlinkSync, mkdtempSync } from 'fs';
import { tmpdir } from 'os';
import { join } from 'path';

function tmpFile(content: string): string {
    const dir = mkdtempSync(join(tmpdir(), 'toml-test-'));
    const path = join(dir, `test-${Date.now()}.toml`);
    writeFileSync(path, content);
    return path;
}

describe('readConfig', () => {
    it('reads basic config sections', async () => {
        const path = tmpFile(`
[server]
state_dir = "~/.local/share/test"

[display]
show_thinking = false
stream_coalesce_ms = 500
        `);

        const config = await readConfig(path);
        expect(config.server?.state_dir).toBe('~/.local/share/test');
        expect(config.display?.show_thinking).toBe(false);
        expect(config.display?.stream_coalesce_ms).toBe(500);

        unlinkSync(path);
    });

    it('preserves env: prefixed values as literal strings', async () => {
        const path = tmpFile(`
[backends.telegram]
enabled = true
bot_token = "env:TELEGRAM_BOT_TOKEN"
        `);

        const config = await readConfig(path);
        expect(config.backends?.telegram?.bot_token).toBe('env:TELEGRAM_BOT_TOKEN');

        unlinkSync(path);
    });

    it('reads web backend config', async () => {
        const path = tmpFile(`
[backends.web]
enabled = true
bind = "0.0.0.0:8080"
dashboard_bind = "0.0.0.0:3001"
dashboard_token = "env:DASHBOARD_TOKEN"
        `);

        const config = await readConfig(path);
        expect(config.backends?.web?.enabled).toBe(true);
        expect(config.backends?.web?.bind).toBe('0.0.0.0:8080');
        expect(config.backends?.web?.dashboard_token).toBe('env:DASHBOARD_TOKEN');

        unlinkSync(path);
    });
});

describe('updateConfigSection', () => {
    it('updates a top-level section', async () => {
        const path = tmpFile(`
[display]
show_thinking = false
stream_coalesce_ms = 500
        `);

        await updateConfigSection(path, 'display', {
            show_thinking: true,
            stream_coalesce_ms: 1000,
        });

        const config = await readConfig(path);
        expect(config.display?.show_thinking).toBe(true);
        expect(config.display?.stream_coalesce_ms).toBe(1000);

        unlinkSync(path);
    });

    it('updates a nested section (backends.web)', async () => {
        const path = tmpFile(`
[backends.web]
enabled = false
        `);

        await updateConfigSection(path, 'backends.web', {
            enabled: true,
            bind: '0.0.0.0:8080',
        });

        const config = await readConfig(path);
        expect(config.backends?.web?.enabled).toBe(true);
        expect(config.backends?.web?.bind).toBe('0.0.0.0:8080');

        unlinkSync(path);
    });

    it('preserves other sections when updating', async () => {
        const path = tmpFile(`
[server]
state_dir = "~/.local/share/test"

[display]
show_thinking = false
        `);

        await updateConfigSection(path, 'display', { show_thinking: true });

        const config = await readConfig(path);
        expect(config.server?.state_dir).toBe('~/.local/share/test');
        expect(config.display?.show_thinking).toBe(true);

        unlinkSync(path);
    });

    it('preserves env: values in other sections when updating', async () => {
        const path = tmpFile(`
[backends.telegram]
enabled = true
bot_token = "env:TELEGRAM_BOT_TOKEN"

[backends.web]
enabled = false
        `);

        await updateConfigSection(path, 'backends.web', { enabled: true });

        const config = await readConfig(path);
        expect(config.backends?.telegram?.bot_token).toBe('env:TELEGRAM_BOT_TOKEN');
        expect(config.backends?.web?.enabled).toBe(true);

        unlinkSync(path);
    });
});
