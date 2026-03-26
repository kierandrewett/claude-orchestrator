import * as React from 'react';
import { Eye, EyeOff } from 'lucide-react';
import { cn } from '../../lib/utils';

export interface InputProps extends React.InputHTMLAttributes<HTMLInputElement> {
    icon?: React.ReactNode;
    error?: boolean;
}

export const Input = React.forwardRef<HTMLInputElement, InputProps>(
    ({ className, icon, error, type, ...props }, ref) => {
        const [showPassword, setShowPassword] = React.useState(false);
        const isPassword = type === 'password';
        const inputType = isPassword && showPassword ? 'text' : type;

        return (
            <div className="relative flex items-center">
                {icon && (
                    <span className="absolute left-2.5 text-zinc-500 pointer-events-none">
                        {icon}
                    </span>
                )}
                <input
                    ref={ref}
                    type={inputType}
                    className={cn(
                        'w-full h-9 bg-zinc-800 border rounded-lg text-sm text-zinc-100 placeholder:text-zinc-500',
                        'focus:outline-none focus:ring-2 focus:ring-zinc-500 focus:border-zinc-500',
                        'transition-colors',
                        error ? 'border-red-500' : 'border-zinc-700',
                        icon ? 'pl-8 pr-3' : 'px-3',
                        isPassword ? 'pr-9' : '',
                        className,
                    )}
                    {...props}
                />
                {isPassword && (
                    <button
                        type="button"
                        onClick={() => setShowPassword(v => !v)}
                        className="absolute right-2.5 text-zinc-500 hover:text-zinc-300 transition-colors"
                        tabIndex={-1}
                    >
                        {showPassword ? <EyeOff size={14} /> : <Eye size={14} />}
                    </button>
                )}
            </div>
        );
    }
);
Input.displayName = 'Input';
