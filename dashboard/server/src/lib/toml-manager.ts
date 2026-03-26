import * as fs from 'fs/promises';
import TOML from '@iarna/toml';

// ── Types ─────────────────────────────────────────────────────────────────────

export interface RawServerConfig {
    state_dir?: string;
    client_bind?: string;
    client_token?: string;
    system_prompt?: string;
}

export interface RawDockerConfig {
    socket?: string;
    default_profile?: string;
    image_prefix?: string;
    idle_timeout_hours?: number;
}

export interface RawDisplayConfig {
    show_thinking?: boolean;
    stream_coalesce_ms?: number;
}

export interface RawTelegramConfig {
    enabled?: boolean;
    bot_token?: string;
    supergroup_id?: number;
    scratchpad_topic_name?: string;
    allowed_users?: number[];
    voice_stt?: string;
    voice_stt_api_key?: string;
    hidden_tools?: string[];
}

export interface RawWebConfig {
    enabled?: boolean;
    bind?: string;
    dashboard_bind?: string;
    dashboard_token?: string;
    dashboard_url?: string;
}

export interface RawBackendsConfig {
    telegram?: RawTelegramConfig;
    web?: RawWebConfig;
}

export interface RawConfig {
    server?: RawServerConfig;
    docker?: RawDockerConfig;
    display?: RawDisplayConfig;
    backends?: RawBackendsConfig;
    [key: string]: unknown;
}

// ── Read/Write ────────────────────────────────────────────────────────────────

export async function readConfig(configPath: string): Promise<RawConfig> {
    const text = await fs.readFile(configPath, 'utf8');
    return TOML.parse(text) as unknown as RawConfig;
}

/**
 * Update a dotted section path in a TOML file.
 * e.g. section = "backends.web", values = { enabled: true, bind: "0.0.0.0:8080" }
 *
 * Strategy: parse the TOML, deep-merge the values into the section, then stringify.
 * We preserve env:VAR_NAME strings because @iarna/toml treats them as regular strings.
 */
export async function updateConfigSection(
    configPath: string,
    section: string,
    values: Record<string, unknown>
): Promise<void> {
    const text = await fs.readFile(configPath, 'utf8');
    const parsed = TOML.parse(text) as Record<string, unknown>;

    // Navigate to / create the section
    const parts = section.split('.');
    let current = parsed;
    for (let i = 0; i < parts.length - 1; i++) {
        const part = parts[i];
        if (!(part in current) || typeof current[part] !== 'object' || current[part] === null) {
            current[part] = {};
        }
        current = current[part] as Record<string, unknown>;
    }

    const lastPart = parts[parts.length - 1];
    if (!(lastPart in current) || typeof current[lastPart] !== 'object' || current[lastPart] === null) {
        current[lastPart] = {};
    }
    const target = current[lastPart] as Record<string, unknown>;

    // Merge values
    for (const [key, value] of Object.entries(values)) {
        if (value === null || value === undefined) {
            delete target[key];
        } else {
            target[key] = value;
        }
    }

    const newText = TOML.stringify(parsed as TOML.JsonMap);
    await fs.writeFile(configPath, newText, 'utf8');
}
