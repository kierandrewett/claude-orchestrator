import * as React from 'react';
import { Slot } from '@radix-ui/react-slot';
import { cn } from '../../lib/utils';

export interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
    variant?: 'default' | 'destructive' | 'outline' | 'ghost' | 'link';
    size?: 'sm' | 'md' | 'lg' | 'icon';
    asChild?: boolean;
}

const variantClasses: Record<NonNullable<ButtonProps['variant']>, string> = {
    default: 'bg-zinc-100 text-zinc-900 hover:bg-zinc-200 border border-transparent',
    destructive: 'bg-red-600 text-white hover:bg-red-700 border border-transparent',
    outline: 'bg-transparent text-zinc-200 hover:bg-zinc-800 border border-zinc-700',
    ghost: 'bg-transparent text-zinc-300 hover:bg-zinc-800 hover:text-zinc-100 border border-transparent',
    link: 'bg-transparent text-zinc-300 hover:text-zinc-100 underline-offset-4 hover:underline border border-transparent p-0 h-auto',
};

const sizeClasses: Record<NonNullable<ButtonProps['size']>, string> = {
    sm: 'h-7 px-2.5 text-xs rounded-md gap-1.5',
    md: 'h-9 px-3.5 text-sm rounded-lg gap-2',
    lg: 'h-10 px-5 text-sm rounded-lg gap-2',
    icon: 'h-8 w-8 p-0 rounded-lg',
};

export const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
    ({ className, variant = 'default', size = 'md', asChild = false, ...props }, ref) => {
        const Comp = asChild ? Slot : 'button';
        return (
            <Comp
                ref={ref}
                className={cn(
                    'inline-flex items-center justify-center font-medium transition-colors',
                    'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-zinc-400',
                    'disabled:opacity-40 disabled:cursor-not-allowed',
                    variantClasses[variant],
                    sizeClasses[size],
                    className,
                )}
                {...props}
            />
        );
    }
);
Button.displayName = 'Button';
