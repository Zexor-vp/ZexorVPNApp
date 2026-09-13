import { useEffect, useState } from 'react';
import Login from './pages/Login';
import Dashboard from './pages/Dashboard';
import { currentSession, type SessionInfo } from './lib/commands';

type Screen =
  | { kind: 'loading' }
  | { kind: 'login' }
  | { kind: 'dashboard'; session: SessionInfo };

export default function App() {
  const [screen, setScreen] = useState<Screen>({ kind: 'loading' });

  useEffect(() => {
    let cancelled = false;
    currentSession()
      .then((session) => {
        if (cancelled) return;
        setScreen(session ? { kind: 'dashboard', session } : { kind: 'login' });
      })
      .catch(() => {
        if (!cancelled) setScreen({ kind: 'login' });
      });
    return () => {
      cancelled = true;
    };
  }, []);

  if (screen.kind === 'loading') {
    return (
      <main className="app-shell loading-shell">
        <span className="spinner spinner-lg" aria-hidden />
      </main>
    );
  }

  if (screen.kind === 'login') {
    return <Login onSuccess={(session) => setScreen({ kind: 'dashboard', session })} />;
  }

  return <Dashboard session={screen.session} onLoggedOut={() => setScreen({ kind: 'login' })} />;
}
