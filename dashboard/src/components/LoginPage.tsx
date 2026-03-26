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
                setError('Invalid token — please try again.');
                return;
            }
            localStorage.setItem('dashboard_token', token);
            void navigate({ to: '/' });
        } catch {
            setError('Connection failed — is the server running?');
        } finally {
            setLoading(false);
        }
    };

    return (
        <div className="flex-1 flex items-center justify-center p-4 bg-zinc-950">
            <div className="w-full max-w-xs">
                {/* Logo */}
                <div className="flex flex-col items-center mb-8">
                    <div className="w-12 h-12 rounded-2xl bg-zinc-900 border border-zinc-800 flex items-center justify-center mb-4 shadow-lg">
                        <Bot size={22} className="text-zinc-300" />
                    </div>
                    <h1 className="text-lg font-semibold text-zinc-100">Claude Orchestrator</h1>
                    <p className="text-sm text-zinc-600 mt-1">Enter your token to continue</p>
                </div>

                {/* Form */}
                <form onSubmit={handleSubmit} className="space-y-3">
                    <div>
                        <Input
                            type="password"
                            placeholder="Dashboard token"
                            value={token}
                            onChange={e => setToken(e.target.value)}
                            icon={<KeyRound size={13} />}
                            autoFocus
                            error={Boolean(error)}
                        />
                        {error && (
                            <p className="mt-1.5 text-xs text-red-400">{error}</p>
                        )}
                    </div>

                    <Button type="submit" className="w-full" disabled={!token || loading}>
                        {loading ? 'Connecting…' : 'Connect'}
                    </Button>
                </form>
            </div>
        </div>
    );
}
