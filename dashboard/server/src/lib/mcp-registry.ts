import * as fs from 'fs/promises';
import * as path from 'path';

// ── Types ─────────────────────────────────────────────────────────────────────

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
}

interface McpRegistryFile {
    custom?: McpServerEntry[];
    disabled?: string[];
}

// ── Built-in servers ──────────────────────────────────────────────────────────

const BUILTIN_SERVERS: McpServerEntry[] = [
    {
        name: 'orchestrator',
        command: 'orchestrator-mcp',
        args: [],
        env: {},
        url: null,
    },
];

// ── Registry path ─────────────────────────────────────────────────────────────

function registryPath(stateDir: string): string {
    return path.join(stateDir, 'mcp_servers.json');
}

async function readRegistry(stateDir: string): Promise<McpRegistryFile> {
    const p = registryPath(stateDir);
    try {
        const text = await fs.readFile(p, 'utf8');
        return JSON.parse(text) as McpRegistryFile;
    } catch {
        return { custom: [], disabled: [] };
    }
}

async function writeRegistry(stateDir: string, reg: McpRegistryFile): Promise<void> {
    const p = registryPath(stateDir);
    await fs.mkdir(stateDir, { recursive: true });
    await fs.writeFile(p, JSON.stringify(reg, null, 2), 'utf8');
}

// ── Exports ───────────────────────────────────────────────────────────────────

export async function listServers(stateDir: string): Promise<McpServer[]> {
    const reg = await readRegistry(stateDir);
    const disabled = new Set(reg.disabled ?? []);

    const builtins: McpServer[] = BUILTIN_SERVERS.map(s => ({
        ...s,
        builtin: true,
        enabled: !disabled.has(s.name),
    }));

    const custom: McpServer[] = (reg.custom ?? []).map(s => ({
        ...s,
        builtin: false,
        enabled: !disabled.has(s.name),
    }));

    return [...builtins, ...custom];
}

export async function addServer(stateDir: string, entry: McpServerEntry): Promise<void> {
    const reg = await readRegistry(stateDir);
    const custom = reg.custom ?? [];
    // Replace if name already exists
    const idx = custom.findIndex(s => s.name === entry.name);
    if (idx >= 0) {
        custom[idx] = entry;
    } else {
        custom.push(entry);
    }
    await writeRegistry(stateDir, { ...reg, custom });
}

export async function removeServer(stateDir: string, name: string): Promise<void> {
    const reg = await readRegistry(stateDir);
    // Only remove custom servers (never builtins)
    const custom = (reg.custom ?? []).filter(s => s.name !== name);
    // Also remove from disabled list
    const disabled = (reg.disabled ?? []).filter(d => d !== name);
    await writeRegistry(stateDir, { ...reg, custom, disabled });
}

export async function toggleServer(stateDir: string, name: string, enabled: boolean): Promise<void> {
    const reg = await readRegistry(stateDir);
    const disabled = new Set(reg.disabled ?? []);
    if (enabled) {
        disabled.delete(name);
    } else {
        disabled.add(name);
    }
    await writeRegistry(stateDir, { ...reg, disabled: Array.from(disabled) });
}
