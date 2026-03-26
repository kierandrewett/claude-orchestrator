import * as React from 'react';
import * as ToggleGroupPrimitive from '@radix-ui/react-toggle-group';
import { cn } from '../../lib/utils';

export const SegmentedControl = React.forwardRef<
    React.ElementRef<typeof ToggleGroupPrimitive.Root>,
    React.ComponentPropsWithoutRef<typeof ToggleGroupPrimitive.Root>
>(({ className, ...props }, ref) => (
    <ToggleGroupPrimitive.Root
        ref={ref}
        className={cn(
            'inline-flex items-center rounded-lg bg-zinc-800 border border-zinc-700 p-0.5 gap-0.5',
            className,
        )}
        {...props}
    />
));
SegmentedControl.displayName = 'SegmentedControl';

export const SegmentedControlItem = React.forwardRef<
    React.ElementRef<typeof ToggleGroupPrimitive.Item>,
    React.ComponentPropsWithoutRef<typeof ToggleGroupPrimitive.Item>
>(({ className, ...props }, ref) => (
    <ToggleGroupPrimitive.Item
        ref={ref}
        className={cn(
            'inline-flex items-center justify-center whitespace-nowrap rounded-md px-3 py-1 text-sm font-medium transition-all',
            'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-zinc-400',
            'disabled:pointer-events-none disabled:opacity-50',
            'data-[state=on]:bg-zinc-600 data-[state=on]:text-zinc-100 data-[state=on]:shadow-sm',
            'text-zinc-400 hover:text-zinc-200',
            className,
        )}
        {...props}
    />
));
SegmentedControlItem.displayName = 'SegmentedControlItem';
