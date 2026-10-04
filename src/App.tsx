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
import { useI18n, useT } from './i18n';
import { currentSession, setNativeLabels } from './lib/commands';

/** Раз в час обновляем подписку: срок, трафик, список серверов. */
const REFRESH_INTERVAL_MS = 60 * 60 * 1000;

/** `guest` — без аккаунта: только своя подписка на главной и вкладка входа. */
type Screen = { kind: 'loading' } | { kind: 'guest' } | { kind: 'app' };

export default function App() {
  const t = useT();
  const { lang } = useI18n();
  const [screen, setScreen] = useState<Screen>({ kind: 'loading' });
  const [tab, setTab] = useState<Tab | 'routing'>('home');
  const [refreshKey, setRefreshKey] = useState(0);
  const updatePhase = useAutoUpdate();

  // Сессия потеряна или пользователь вышел: остаёмся в приложении как гость, на вкладке входа.
  // Меню трея и системные уведомления рисует оболочка (Rust) — отдаём ей тексты на выбранном языке.
  useEffect(() => {
    void setNativeLabels({
      support_reply: t('Ответ поддержки'),
      tray_open: t('Открыть Zexor VPN'),
      tray_disconnect: t('Отключить VPN'),
      tray_quit: t('Выйти (VPN отключится)'),
      language: lang,
    }).catch(() => undefined);
  }, [t, lang]);

  const goToLogin = useCallback(() => {
    setTab('login');
    setScreen({ kind: 'guest' });
  }, []);

  useEffect(() => {
    let cancelled = false;
    currentSession()
      .then((session) => {
        if (!cancelled) setScreen(session ? { kind: 'app' } : { kind: 'guest' });
      })
      .catch((err) => {
        // Нет сети — это не «вышел из аккаунта»: токен сохранён, просто сейчас его не проверить. Остаёмся в приложении
        // (страницы покажут, что связи нет), иначе без интернета пользователя выбрасывало бы на экран гостя.
        const offline = (err as { kind?: string } | null)?.kind === 'Network';
        if (!cancelled) setScreen(offline ? { kind: 'app' } : { kind: 'guest' });
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

  if (screen.kind === 'guest') {
    return (
      <>
        <UpdateOverlay phase={updatePhase} />
        {tab === 'routing' ? (
          <RoutingPage onBack={() => setTab('home')} />
        ) : tab === 'login' ? (
          <Login
            onSuccess={() => {
              setTab('home');
              setScreen({ kind: 'app' });
            }}
          />
        ) : (
          <Home guest refreshKey={refreshKey} onOpenSubscription={() => setTab('login')} onOpenRouting={() => setTab('routing')} />
        )}
        <BottomNav active={tab === 'routing' ? 'home' : tab} onChange={setTab} guest />
      </>
    );
  }

  return (
    <SupportUnreadProvider enabled>
      <Shell tab={tab} setTab={setTab} refreshKey={refreshKey} updatePhase={updatePhase} onLoggedOut={() => {
        setTab('home');
        setScreen({ kind: 'guest' });
      }} />
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
