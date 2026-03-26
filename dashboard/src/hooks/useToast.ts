import * as React from 'react';
import type { ToastVariant } from '../components/ui/toast';

interface ToastData {
    id: string;
    variant?: ToastVariant;
    title?: string;
    description?: string;
    duration?: number;
}

interface ToastState {
    toasts: ToastData[];
}

type ToastAction =
    | { type: 'ADD'; toast: ToastData }
    | { type: 'REMOVE'; id: string };

function toastReducer(state: ToastState, action: ToastAction): ToastState {
    switch (action.type) {
        case 'ADD':
            return { toasts: [action.toast, ...state.toasts].slice(0, 5) };
        case 'REMOVE':
            return { toasts: state.toasts.filter(t => t.id !== action.id) };
    }
}

// Singleton store so toasts can be dispatched from anywhere
let dispatch: React.Dispatch<ToastAction> | null = null;
let state: ToastState = { toasts: [] };
const listeners: Array<(state: ToastState) => void> = [];

function notify(s: ToastState) {
    state = s;
    listeners.forEach(l => l(s));
}

function globalDispatch(action: ToastAction) {
    const next = toastReducer(state, action);
    notify(next);
    if (dispatch) dispatch(action);
}

export function toast(opts: Omit<ToastData, 'id'>) {
    const id = Math.random().toString(36).slice(2);
    const data: ToastData = { id, ...opts };
    globalDispatch({ type: 'ADD', toast: data });
    const duration = opts.duration ?? 4000;
    if (duration > 0) {
        setTimeout(() => globalDispatch({ type: 'REMOVE', id }), duration);
    }
    return id;
}

export function useToastStore() {
    const [localState, setLocalState] = React.useState<ToastState>(state);

    React.useEffect(() => {
        const handler = (s: ToastState) => setLocalState({ ...s });
        listeners.push(handler);
        return () => {
            const idx = listeners.indexOf(handler);
            if (idx >= 0) listeners.splice(idx, 1);
        };
    }, []);

    React.useEffect(() => {
        dispatch = React.useReducer(toastReducer, state)[1];
    });

    return localState;
}

export function useToast() {
    return { toast };
}
