import { render, screen } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { Button } from '../components/ui/button';
import { Badge } from '../components/ui/badge';
import { Input } from '../components/ui/input';
import { Skeleton } from '../components/ui/skeleton';

describe('Button', () => {
    it('renders with default variant', () => {
        render(<Button>Click me</Button>);
        expect(screen.getByRole('button', { name: 'Click me' })).toBeInTheDocument();
    });

    it('is disabled when disabled prop is set', () => {
        render(<Button disabled>Click me</Button>);
        expect(screen.getByRole('button')).toBeDisabled();
    });

    it('renders with destructive variant', () => {
        render(<Button variant="destructive">Delete</Button>);
        const btn = screen.getByRole('button', { name: 'Delete' });
        expect(btn).toBeInTheDocument();
        expect(btn.className).toContain('bg-red');
    });

    it('renders with ghost variant', () => {
        render(<Button variant="ghost">Ghost</Button>);
        expect(screen.getByRole('button', { name: 'Ghost' })).toBeInTheDocument();
    });

    it('renders with sm size', () => {
        render(<Button size="sm">Small</Button>);
        const btn = screen.getByRole('button', { name: 'Small' });
        expect(btn.className).toContain('h-7');
    });
});

describe('Badge', () => {
    it('renders children', () => {
        render(<Badge>Running</Badge>);
        expect(screen.getByText('Running')).toBeInTheDocument();
    });

    it('renders success variant', () => {
        render(<Badge variant="success">Active</Badge>);
        const badge = screen.getByText('Active');
        expect(badge.className).toContain('emerald');
    });

    it('renders warning variant', () => {
        render(<Badge variant="warning">Warning</Badge>);
        const badge = screen.getByText('Warning');
        expect(badge.className).toContain('amber');
    });

    it('renders destructive variant', () => {
        render(<Badge variant="destructive">Error</Badge>);
        const badge = screen.getByText('Error');
        expect(badge.className).toContain('red');
    });
});

describe('Input', () => {
    it('renders an input element', () => {
        render(<Input placeholder="Enter text" />);
        expect(screen.getByPlaceholderText('Enter text')).toBeInTheDocument();
    });

    it('accepts value changes', () => {
        render(<Input defaultValue="test" />);
        expect(screen.getByDisplayValue('test')).toBeInTheDocument();
    });
});

describe('Skeleton', () => {
    it('renders with animate-pulse class', () => {
        const { container } = render(<Skeleton className="h-10 w-24" />);
        const el = container.firstChild as HTMLElement;
        expect(el.className).toContain('animate-pulse');
        expect(el.className).toContain('h-10');
    });
});
