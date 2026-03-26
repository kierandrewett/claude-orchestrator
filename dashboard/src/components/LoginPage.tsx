import * as React from 'react';
import { useNavigate } from '@tanstack/react-router';
import { Bot, KeyRound } from 'lucide-react';
import { Input } from './ui/input';
import { Button } from './ui/button';

export function LoginPage() {
    const [token, setToken] = React.useState('');
    const [error, setError] = React.useState('');
    const [loading, setLoading] = React.useState(false);
    const navigate = useNavigate();

    const handleSubmit = async (e: React.FormEvent) => {
        e.preventDefault();
        setLoading(true);
        setError('');
        try {
            const res = await fetch('/trpc/tasks.list', {
                headers: { Authorization: `Bearer ${token}` },
            });
            if (res.status === 401) {
                setError('Invalid token. Please try again.');
                return;
            }
            localStorage.setItem('dashboard_token', token);
            void navigate({ to: '/' });
        } catch {
            setError('Connection failed. Is the server running?');
        } finally {
            setLoading(false);
        }
    };

    return (
        <div className="flex-1 flex items-center justify-center p-4">
            <div className="w-full max-w-sm">
                <div className="flex flex-col items-center mb-8">
                    <div className="w-14 h-14 rounded-2xl bg-zinc-800 border border-zinc-700 flex items-center justify-center mb-4">
                        <Bot size={28} className="text-zinc-300" />
                    </div>
                    <h1 className="text-xl font-bold text-zinc-100">Dashboard Access</h1>
                    <p className="text-sm text-zinc-500 mt-1 text-center">
                        Enter your dashboard token to continue
                    </p>
                </div>

                <form onSubmit={handleSubmit} className="space-y-4">
                    <div>
                        <Input
                            type="password"
                            placeholder="Dashboard token"
                            value={token}
                            onChange={e => setToken(e.target.value)}
                            icon={<KeyRound size={14} />}
                            autoFocus
                            error={Boolean(error)}
                        />
                        {error && (
                            <p className="mt-1.5 text-xs text-red-400">{error}</p>
                        )}
                    </div>

                    <Button
                        type="submit"
                        className="w-full"
                        disabled={!token || loading}
                    >
                        {loading ? 'Connecting...' : 'Connect'}
                    </Button>
                </form>
            </div>
        </div>
    );
}
