import { useState } from 'react';
import { Routes, Route } from 'react-router-dom';
import { useWebSocket } from './hooks/useWebSocket';
import { Header } from './components/layout/Header';
import { Sidebar } from './components/layout/Sidebar';
import { SessionViewer } from './components/viewer/SessionViewer';

function HomePage() {
    return (
        <div className="flex flex-col items-center justify-center h-full text-zinc-500 gap-3">
            <div className="text-5xl">⌘</div>
            <p className="text-lg font-medium text-zinc-400">Claude Orchestrator</p>
            <p className="text-sm">Select a session from the sidebar or create a new one.</p>
        </div>
    );
}

export default function App() {
    useWebSocket();
    const [sidebarOpen, setSidebarOpen] = useState(false);

    return (
        <div className="flex flex-col h-screen bg-zinc-950 text-zinc-100 overflow-hidden">
            <Header onMenuClick={() => setSidebarOpen((v) => !v)} />
            <div className="flex flex-1 overflow-hidden">
                {/* Mobile sidebar overlay */}
                {sidebarOpen && (
                    <div
                        className="fixed inset-0 z-20 bg-black/60 lg:hidden"
                        onClick={() => setSidebarOpen(false)}
                    />
                )}

                {/* Sidebar */}
                <aside
                    className={[
                        'fixed lg:static inset-y-0 left-0 z-30 w-72 bg-zinc-900 border-r border-zinc-800',
                        'transform transition-transform duration-200 ease-in-out',
                        'lg:transform-none lg:translate-x-0',
                        sidebarOpen ? 'translate-x-0' : '-translate-x-full',
                        'flex flex-col',
                        // Account for header on mobile
                        'top-12 lg:top-0',
                    ].join(' ')}
                >
                    <Sidebar onNavigate={() => setSidebarOpen(false)} />
                </aside>

                {/* Main content */}
                <main className="flex-1 overflow-hidden flex flex-col">
                    <Routes>
                        <Route path="/" element={<HomePage />} />
                        <Route path="/session/:id" element={<SessionViewer />} />
                    </Routes>
                </main>
            </div>
        </div>
    );
}
