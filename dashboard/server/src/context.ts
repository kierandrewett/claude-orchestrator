import type { CreateFastifyContextOptions } from '@trpc/server/adapters/fastify';
import type { Context } from './trpc.js';

export async function createContext({ req, res }: CreateFastifyContextOptions): Promise<Context> {
    return { req, res };
}
