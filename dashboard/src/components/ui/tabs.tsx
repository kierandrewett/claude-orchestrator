import * as React from 'react';
import * as TabsPrimitive from '@radix-ui/react-tabs';
import { cn } from '../../lib/utils';

export const Tabs = TabsPrimitive.Root;

export function TabsList({
    className,
    ...props
}: React.ComponentPropsWithoutRef<typeof TabsPrimitive.List>) {
    return (
        <TabsPrimitive.List
            className={cn(
                'inline-flex h-9 items-center justify-center rounded-lg bg-zinc-800 p-1 text-zinc-400',
                className,
            )}
            {...props}
        />
    );
}

export function TabsTrigger({
    className,
    ...props
}: React.ComponentPropsWithoutRef<typeof TabsPrimitive.Trigger>) {
    return (
        <TabsPrimitive.Trigger
            className={cn(
                'inline-flex items-center justify-center whitespace-nowrap rounded-md px-3 py-1 text-sm font-medium transition-all',
                'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-zinc-400',
                'disabled:pointer-events-none disabled:opacity-50',
                'data-[state=active]:bg-zinc-700 data-[state=active]:text-zinc-100 data-[state=active]:shadow-sm',
                'text-zinc-400 hover:text-zinc-200',
                className,
            )}
            {...props}
        />
    );
}

export function TabsContent({
    className,
    ...props
}: React.ComponentPropsWithoutRef<typeof TabsPrimitive.Content>) {
    return (
        <TabsPrimitive.Content
            className={cn('mt-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-zinc-400', className)}
            {...props}
        />
    );
}
