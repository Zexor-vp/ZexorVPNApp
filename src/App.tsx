import { useCallback, useEffect, useState } from 'react';
import BottomNav, { type Tab } from './components/BottomNav';
import UpdateOverlay from './components/UpdateOverlay';
import AdblockPage from './pages/AdblockPage';
import Home from './pages/Home';
import Login from './pages/Login';
import ProfilePage from './pages/ProfilePage';
import RoutingPage from './pages/RoutingPage';
import SubscriptionPage from './pages/SubscriptionPage';
import SupportPage from './pages/SupportPage';
import { setAuthLostHandler } from './hooks/useAsync';
import { useAutoUpdate, type UpdatePhase } from './hooks/useAutoUpdate';
import { SupportUnreadProvider, useSupportUnread } from './hooks/useSupportUnread';
import { currentSession } from './lib/commands';

/** Раз в час обновляем подписку: срок, трафик, список серверов. */
const REFRESH_INTERVAL_MS = 60 * 60 * 1000;

type Screen = { kind: 'loading' } | { kind: 'login' } | { kind: 'app' };

export default function App() {
  const [screen, setScreen] = useState<Screen>({ kind: 'loading' });
  const [tab, setTab] = useState<Tab | 'routing'>('home');
  const [refreshKey, setRefreshKey] = useState(0);
  const updatePhase = useAutoUpdate();

  const goToLogin = useCallback(() => setScreen({ kind: 'login' }), []);

  useEffect(() => {
    let cancelled = false;
    currentSession()
      .then((session) => {
        if (!cancelled) setScreen(session ? { kind: 'app' } : { kind: 'login' });
      })
      .catch(() => {
        if (!cancelled) setScreen({ kind: 'login' });
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    const timer = window.setInterval(() => setRefreshKey((key) => key + 1), REFRESH_INTERVAL_MS);
    return () => window.clearInterval(timer);
  }, []);

  // Любая страница, получив «сессия истекла», возвращает на экран входа.
  useEffect(() => {
    setAuthLostHandler(goToLogin);
    return () => setAuthLostHandler(null);
  }, [goToLogin]);

  if (screen.kind === 'loading') {
    return (
      <main className="app-shell loading-shell">
        <span className="spinner spinner-lg" aria-hidden />
      </main>
    );
  }

  if (screen.kind === 'login') {
    return (
      <Login
        onSuccess={() => {
          setTab('home');
          setScreen({ kind: 'app' });
        }}
      />
    );
  }

  return (
    <SupportUnreadProvider enabled>
      <Shell tab={tab} setTab={setTab} refreshKey={refreshKey} updatePhase={updatePhase} onLoggedOut={goToLogin} />
    </SupportUnreadProvider>
  );
}

interface ShellProps {
  tab: Tab | 'routing';
  setTab: (tab: Tab | 'routing') => void;
  refreshKey: number;
  updatePhase: UpdatePhase;
  onLoggedOut: () => void;
}

function Shell({ tab, setTab, refreshKey, updatePhase, onLoggedOut }: ShellProps) {
  const { total } = useSupportUnread();
  return (
    <>
      <UpdateOverlay phase={updatePhase} />
      {tab === 'home' && (
        <Home refreshKey={refreshKey} onOpenSubscription={() => setTab('subscription')} onOpenRouting={() => setTab('routing')} />
      )}
      {tab === 'routing' && <RoutingPage onBack={() => setTab('home')} />}
      {tab === 'subscription' && <SubscriptionPage />}
      {tab === 'adblock' && <AdblockPage />}
      {tab === 'support' && <SupportPage />}
      {tab === 'profile' && <ProfilePage onLoggedOut={onLoggedOut} />}
      <BottomNav active={tab === 'routing' ? 'home' : tab} onChange={setTab} dots={{ support: total > 0 }} />
    </>
  );
}
