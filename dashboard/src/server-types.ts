/**
 * Client-side mirror of server types.
 * These must stay in sync with dashboard/server/src/lib/*.ts
 */

export type TaskState = 'Running' | 'Hibernated' | 'Dead';

export interface TaskInfo {
    id: string;
    name: string;
    profile: string;
    state: TaskState;
    created_at: string;
    input_tokens: number;
    output_tokens: number;
    cost_usd: number;
    turns: number;
    last_activity: string;
}

export interface McpServerEntry {
    name: string;
    command?: string;
    args?: string[];
    env?: Record<string, string>;
    url?: string | null;
    disabled?: boolean;
}

export interface McpServer extends McpServerEntry {
    builtin: boolean;
    enabled: boolean;
    /** null = no running session to check; true = tools loaded; false = failed to connect */
    connected?: boolean | null;
}

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
