import { useEffect } from 'react';

type KeyboardHandler = (e: KeyboardEvent) => void;

export function useKeyboard(handler: KeyboardHandler) {
    useEffect(() => {
        window.addEventListener('keydown', handler);
        return () => window.removeEventListener('keydown', handler);
    }, [handler]);
}

export function useGlobalShortcut(
    key: string,
    opts: { ctrl?: boolean; meta?: boolean; shift?: boolean; alt?: boolean },
    handler: () => void,
) {
    useEffect(() => {
        const listener = (e: KeyboardEvent) => {
            const tag = (e.target as Element)?.tagName?.toLowerCase();
            const isInput = tag === 'input' || tag === 'textarea' || tag === 'select';

            const metaMatch = opts.meta ? e.metaKey || e.ctrlKey : true;
            const ctrlMatch = opts.ctrl ? e.ctrlKey : true;
            const shiftMatch = opts.shift ? e.shiftKey : !opts.shift || false;
            const altMatch = opts.alt ? e.altKey : true;

            if (
                e.key.toLowerCase() === key.toLowerCase() &&
                metaMatch &&
                (!opts.ctrl || ctrlMatch) &&
                (!opts.shift || shiftMatch) &&
                (!opts.alt || altMatch)
            ) {
                // For non-modified keys, don't trigger in inputs
                if (!opts.meta && !opts.ctrl && isInput) return;
                e.preventDefault();
                handler();
            }
        };
        window.addEventListener('keydown', listener);
        return () => window.removeEventListener('keydown', listener);
    }, [key, opts.ctrl, opts.meta, opts.shift, opts.alt, handler]);
}
