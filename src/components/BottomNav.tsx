import type { ReactNode } from 'react';
import { ChatIcon, HomeIcon, ShieldIcon, SparklesIcon, UserIcon } from './Icons';

export type Tab = 'home' | 'subscription' | 'adblock' | 'support' | 'profile' | 'login';

const ITEMS: { tab: Tab; label: string; icon: ReactNode }[] = [
  { tab: 'home', label: 'Главная', icon: <HomeIcon /> },
  { tab: 'subscription', label: 'Подписка', icon: <SparklesIcon /> },
  { tab: 'adblock', label: 'MyBlock', icon: <ShieldIcon /> },
  { tab: 'support', label: 'Поддержка', icon: <ChatIcon /> },
  { tab: 'profile', label: 'Профиль', icon: <UserIcon /> },
];

/** Без аккаунта доступны только главная (своя подписка) и вход. */
const GUEST_ITEMS: { tab: Tab; label: string; icon: ReactNode }[] = [
  { tab: 'home', label: 'Главная', icon: <HomeIcon /> },
  { tab: 'login', label: 'Вход', icon: <UserIcon /> },
];

interface Props {
  active: Tab;
  onChange: (tab: Tab) => void;
  /** Вкладки с маленьким красным кружком (непрочитанное). */
  dots?: Partial<Record<Tab, boolean>>;
  /** Пользователь не вошёл в аккаунт — показываем только «Главная» и «Вход». */
  guest?: boolean;
}

export default function BottomNav({ active, onChange, dots, guest }: Props) {
  return (
    <nav className="bottom-nav" aria-label="Разделы">
      {(guest ? GUEST_ITEMS : ITEMS).map((item) => (
        <button
          key={item.tab}
          type="button"
          className={`nav-item ${active === item.tab ? 'nav-item-active' : ''}`}
          aria-current={active === item.tab ? 'page' : undefined}
          onClick={() => onChange(item.tab)}
        >
          <span className="nav-icon">
            {item.icon}
            {dots?.[item.tab] && active !== item.tab && <span className="nav-dot" aria-label="Есть новые" />}
          </span>
          {item.label}
        </button>
      ))}
    </nav>
  );
}
