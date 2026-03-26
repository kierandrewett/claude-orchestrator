import { describe, it, expect } from 'vitest';
import { formatDuration, formatCost, formatTokens, getStatusDot } from '../lib/utils';

describe('formatDuration', () => {
    it('returns — when no start time', () => {
        expect(formatDuration(undefined)).toBe('—');
    });

    it('formats seconds only', () => {
        const start = new Date(Date.now() - 45_000).toISOString();
        const end = new Date().toISOString();
        expect(formatDuration(start, end)).toBe('45s');
    });

    it('formats minutes and seconds', () => {
        const start = new Date(Date.now() - 125_000).toISOString();
        const end = new Date().toISOString();
        expect(formatDuration(start, end)).toBe('2m 5s');
    });
});

describe('formatCost', () => {
    it('returns — for undefined', () => {
        expect(formatCost(undefined)).toBe('—');
    });

    it('returns $0.00 for zero', () => {
        expect(formatCost(0)).toBe('$0.00');
    });

    it('formats small costs with more decimal places', () => {
        expect(formatCost(0.0005)).toMatch(/^\$0\.0005/);
    });
});

describe('formatTokens', () => {
    it('returns 0 for zero', () => {
        expect(formatTokens(0)).toBe('0');
    });

    it('formats thousands with k suffix', () => {
        expect(formatTokens(1500)).toBe('1.5k');
    });

    it('formats millions with M suffix', () => {
        expect(formatTokens(1_500_000)).toBe('1.50M');
    });
});

describe('getStatusDot', () => {
    it('returns emerald for Running', () => {
        expect(getStatusDot('Running')).toContain('emerald');
    });

    it('returns amber for Hibernated', () => {
        expect(getStatusDot('Hibernated')).toContain('amber');
    });

    it('returns zinc for Dead', () => {
        expect(getStatusDot('Dead')).toContain('zinc');
    });

    it('returns zinc for unknown state', () => {
        expect(getStatusDot('unknown')).toContain('zinc');
    });
});
