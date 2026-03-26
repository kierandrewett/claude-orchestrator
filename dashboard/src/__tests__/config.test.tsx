import { render, screen } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import { EnvBadge, isEnvValue } from '../components/config/EnvBadge';

// Mock tRPC hooks
vi.mock('../api/trpc', () => ({
    trpc: {
        config: {
            updateSection: {
                useMutation: () => ({
                    mutate: vi.fn(),
                    isPending: false,
                }),
            },
        },
    },
}));

describe('EnvBadge', () => {
    it('displays env: prefix and variable name', () => {
        render(<EnvBadge value="env:MY_TOKEN" />);
        expect(screen.getByText('env:')).toBeInTheDocument();
        expect(screen.getByText('MY_TOKEN')).toBeInTheDocument();
    });

    it('strips env: from display', () => {
        render(<EnvBadge value="env:TELEGRAM_BOT_TOKEN" />);
        expect(screen.getByText('TELEGRAM_BOT_TOKEN')).toBeInTheDocument();
    });
});

describe('isEnvValue', () => {
    it('returns true for env: prefixed strings', () => {
        expect(isEnvValue('env:FOO')).toBe(true);
        expect(isEnvValue('env:SOME_TOKEN')).toBe(true);
    });

    it('returns false for regular values', () => {
        expect(isEnvValue('my-plain-token')).toBe(false);
        expect(isEnvValue(null)).toBe(false);
        expect(isEnvValue(undefined)).toBe(false);
        expect(isEnvValue(123)).toBe(false);
    });
});
