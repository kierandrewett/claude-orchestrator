import * as React from 'react';
import * as SelectPrimitive from '@radix-ui/react-select';
import { Check, ChevronDown, ChevronUp } from 'lucide-react';
import { cn } from '../../lib/utils';

export const Select = SelectPrimitive.Root;
export const SelectValue = SelectPrimitive.Value;
export const SelectGroup = SelectPrimitive.Group;

export function SelectTrigger({
    className,
    children,
    ...props
}: React.ComponentPropsWithoutRef<typeof SelectPrimitive.Trigger>) {
    return (
        <SelectPrimitive.Trigger
            className={cn(
                'flex h-9 w-full items-center justify-between rounded-lg border border-zinc-700 bg-zinc-800 px-3 py-2 text-sm text-zinc-100 placeholder:text-zinc-500',
                'focus:outline-none focus:ring-2 focus:ring-zinc-500 focus:border-zinc-500',
                'disabled:opacity-40 disabled:cursor-not-allowed',
                '[&>span]:line-clamp-1',
                className,
            )}
            {...props}
        >
            {children}
            <SelectPrimitive.Icon asChild>
                <ChevronDown className="h-4 w-4 opacity-50 shrink-0" />
            </SelectPrimitive.Icon>
        </SelectPrimitive.Trigger>
    );
}

export function SelectContent({
    className,
    children,
    position = 'popper',
    ...props
}: React.ComponentPropsWithoutRef<typeof SelectPrimitive.Content>) {
    return (
        <SelectPrimitive.Portal>
            <SelectPrimitive.Content
                className={cn(
                    'relative z-50 max-h-96 min-w-[8rem] overflow-hidden rounded-lg border border-zinc-700 bg-zinc-800 shadow-xl',
                    'data-[state=open]:animate-in data-[state=closed]:animate-out',
                    'data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0',
                    'data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95',
                    'data-[side=bottom]:slide-in-from-top-2 data-[side=top]:slide-in-from-bottom-2',
                    position === 'popper' && 'data-[side=bottom]:translate-y-1 data-[side=top]:-translate-y-1',
                    className,
                )}
                position={position}
                {...props}
            >
                <SelectPrimitive.ScrollUpButton className="flex cursor-default items-center justify-center py-1">
                    <ChevronUp className="h-4 w-4 text-zinc-400" />
                </SelectPrimitive.ScrollUpButton>
                <SelectPrimitive.Viewport
                    className={cn('p-1', position === 'popper' && 'w-full min-w-[var(--radix-select-trigger-width)]')}
                >
                    {children}
                </SelectPrimitive.Viewport>
                <SelectPrimitive.ScrollDownButton className="flex cursor-default items-center justify-center py-1">
                    <ChevronDown className="h-4 w-4 text-zinc-400" />
                </SelectPrimitive.ScrollDownButton>
            </SelectPrimitive.Content>
        </SelectPrimitive.Portal>
    );
}

export function SelectItem({
    className,
    children,
    ...props
}: React.ComponentPropsWithoutRef<typeof SelectPrimitive.Item>) {
    return (
        <SelectPrimitive.Item
            className={cn(
                'relative flex w-full cursor-default select-none items-center rounded-md py-1.5 pl-8 pr-2 text-sm text-zinc-200',
                'focus:bg-zinc-700 focus:text-zinc-100 focus:outline-none',
                'data-[disabled]:pointer-events-none data-[disabled]:opacity-40',
                className,
            )}
            {...props}
        >
            <span className="absolute left-2 flex h-3.5 w-3.5 items-center justify-center">
                <SelectPrimitive.ItemIndicator>
                    <Check className="h-4 w-4 text-zinc-400" />
                </SelectPrimitive.ItemIndicator>
            </span>
            <SelectPrimitive.ItemText>{children}</SelectPrimitive.ItemText>
        </SelectPrimitive.Item>
    );
}

export function SelectLabel({
    className,
    ...props
}: React.ComponentPropsWithoutRef<typeof SelectPrimitive.Label>) {
    return (
        <SelectPrimitive.Label
            className={cn('py-1.5 pl-8 pr-2 text-xs font-semibold text-zinc-500', className)}
            {...props}
        />
    );
}

export function SelectSeparator({
    className,
    ...props
}: React.ComponentPropsWithoutRef<typeof SelectPrimitive.Separator>) {
    return (
        <SelectPrimitive.Separator
            className={cn('-mx-1 my-1 h-px bg-zinc-700', className)}
            {...props}
        />
    );
}
