import * as React from 'react';
import * as ToastPrimitive from '@radix-ui/react-toast';
import { X, CheckCircle2, AlertCircle, Info } from 'lucide-react';
import { cn } from '../../lib/utils';

export const ToastProvider = ToastPrimitive.Provider;
export const ToastViewport = React.forwardRef<
    React.ElementRef<typeof ToastPrimitive.Viewport>,
    React.ComponentPropsWithoutRef<typeof ToastPrimitive.Viewport>
>(({ className, ...props }, ref) => (
    <ToastPrimitive.Viewport
        ref={ref}
        className={cn(
            'fixed bottom-4 right-4 z-[100] flex max-h-screen w-full max-w-sm flex-col gap-2',
            className,
        )}
        {...props}
    />
));
ToastViewport.displayName = ToastPrimitive.Viewport.displayName;

export type ToastVariant = 'default' | 'success' | 'error' | 'info';

export interface ToastProps extends React.ComponentPropsWithoutRef<typeof ToastPrimitive.Root> {
    variant?: ToastVariant;
    title?: string;
    description?: string;
}

const variantClasses: Record<ToastVariant, string> = {
    default: 'border-zinc-700 bg-zinc-900',
    success: 'border-emerald-500/30 bg-zinc-900',
    error: 'border-red-500/30 bg-zinc-900',
    info: 'border-blue-500/30 bg-zinc-900',
};

const variantIcons: Record<ToastVariant, React.ReactNode> = {
    default: null,
    success: <CheckCircle2 className="w-4 h-4 text-emerald-400 shrink-0" />,
    error: <AlertCircle className="w-4 h-4 text-red-400 shrink-0" />,
    info: <Info className="w-4 h-4 text-blue-400 shrink-0" />,
};

export const Toast = React.forwardRef<
    React.ElementRef<typeof ToastPrimitive.Root>,
    ToastProps
>(({ className, variant = 'default', title, description, children, ...props }, ref) => (
    <ToastPrimitive.Root
        ref={ref}
        className={cn(
            'relative flex items-start gap-3 w-full rounded-xl border p-3 shadow-xl',
            'data-[state=open]:animate-in data-[state=closed]:animate-out',
            'data-[state=closed]:fade-out-80 data-[state=closed]:slide-out-to-right-full',
            'data-[state=open]:slide-in-from-bottom-full',
            variantClasses[variant],
            className,
        )}
        {...props}
    >
        {variantIcons[variant]}
        <div className="flex-1 min-w-0">
            {title && (
                <ToastPrimitive.Title className="text-sm font-medium text-zinc-100">
                    {title}
                </ToastPrimitive.Title>
            )}
            {description && (
                <ToastPrimitive.Description className="text-xs text-zinc-400 mt-0.5">
                    {description}
                </ToastPrimitive.Description>
            )}
            {children}
        </div>
        <ToastPrimitive.Close className="shrink-0 p-0.5 rounded text-zinc-600 hover:text-zinc-300 transition-colors">
            <X size={14} />
        </ToastPrimitive.Close>
    </ToastPrimitive.Root>
));
Toast.displayName = ToastPrimitive.Root.displayName;
