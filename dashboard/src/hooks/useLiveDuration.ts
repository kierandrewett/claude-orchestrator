import { useState, useEffect } from 'react';
import { formatDuration } from '../lib/utils';

export function useLiveDuration(startedAt?: string, endedAt?: string, isRunning?: boolean): string {
    const [display, setDisplay] = useState(() => formatDuration(startedAt, endedAt));

    useEffect(() => {
        if (!isRunning || endedAt) {
            setDisplay(formatDuration(startedAt, endedAt));
            return;
        }

        setDisplay(formatDuration(startedAt, endedAt));
        const interval = setInterval(() => {
            setDisplay(formatDuration(startedAt, undefined));
        }, 1000);

        return () => clearInterval(interval);
    }, [startedAt, endedAt, isRunning]);

    return display;
}
