import * as React from 'react';
import { Command as CommandPrimitive } from 'cmdk';
import { Search } from 'lucide-react';
import { cn } from '../../lib/utils';

export const Command = React.forwardRef<
    React.ElementRef<typeof CommandPrimitive>,
    React.ComponentPropsWithoutRef<typeof CommandPrimitive>
>(({ className, ...props }, ref) => (
    <CommandPrimitive
        ref={ref}
        className={cn(
            'flex h-full w-full flex-col overflow-hidden rounded-xl bg-zinc-900',
            className,
        )}
        {...props}
    />
));
Command.displayName = CommandPrimitive.displayName;

export function CommandInput({
    className,
    ...props
}: React.ComponentPropsWithoutRef<typeof CommandPrimitive.Input>) {
    return (
        <div className="flex items-center border-b border-zinc-800 px-3" cmdk-input-wrapper="">
            <Search className="mr-2 h-4 w-4 shrink-0 text-zinc-500" />
            <CommandPrimitive.Input
                className={cn(
                    'flex h-11 w-full rounded-md bg-transparent py-3 text-sm text-zinc-100 placeholder:text-zinc-500',
                    'outline-none disabled:cursor-not-allowed disabled:opacity-50',
                    className,
                )}
                {...props}
            />
        </div>
    );
}

export function CommandList({
    className,
    ...props
}: React.ComponentPropsWithoutRef<typeof CommandPrimitive.List>) {
    return (
        <CommandPrimitive.List
            className={cn('max-h-[300px] overflow-y-auto overflow-x-hidden', className)}
            {...props}
        />
    );
}

export function CommandEmpty({
    ...props
}: React.ComponentPropsWithoutRef<typeof CommandPrimitive.Empty>) {
    return (
        <CommandPrimitive.Empty
            className="py-6 text-center text-sm text-zinc-500"
            {...props}
        />
    );
}

export function CommandGroup({
    className,
    ...props
}: React.ComponentPropsWithoutRef<typeof CommandPrimitive.Group>) {
    return (
        <CommandPrimitive.Group
            className={cn(
                'overflow-hidden p-1 text-zinc-200 [&_[cmdk-group-heading]]:px-2 [&_[cmdk-group-heading]]:py-1.5 [&_[cmdk-group-heading]]:text-xs [&_[cmdk-group-heading]]:font-medium [&_[cmdk-group-heading]]:text-zinc-500',
                className,
            )}
            {...props}
        />
    );
}

export function CommandSeparator({
    className,
    ...props
}: React.ComponentPropsWithoutRef<typeof CommandPrimitive.Separator>) {
    return (
        <CommandPrimitive.Separator
            className={cn('-mx-1 h-px bg-zinc-800', className)}
            {...props}
        />
    );
}

export function CommandItem({
    className,
    ...props
}: React.ComponentPropsWithoutRef<typeof CommandPrimitive.Item>) {
    return (
        <CommandPrimitive.Item
            className={cn(
                'relative flex cursor-default select-none items-center gap-2 rounded-lg px-2 py-1.5 text-sm text-zinc-200',
                'data-[selected=true]:bg-zinc-800 data-[selected=true]:text-zinc-100',
                'data-[disabled=true]:pointer-events-none data-[disabled=true]:opacity-50',
                'outline-none',
                className,
            )}
            {...props}
        />
    );
}

export function CommandShortcut({ className, ...props }: React.HTMLAttributes<HTMLSpanElement>) {
    return (
        <span
            className={cn('ml-auto text-xs tracking-widest text-zinc-500', className)}
            {...props}
        />
    );
}
