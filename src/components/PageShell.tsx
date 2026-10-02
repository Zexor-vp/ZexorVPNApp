import type { ReactNode } from 'react';
import logo from '../assets/logo.png';

interface Props {
  /** Кнопки справа в шапке (например, «+» на главной). */
  actions?: ReactNode;
  children: ReactNode;
}

/** Общий каркас страницы: шапка с логотипом и прокручиваемое содержимое. */
export default function PageShell({ actions, children }: Props) {
  return (
    <div className="frame">
      <header className="topbar">
        <div className="brand">
          <img className="brand-logo" src={logo} alt="" width={36} height={36} />
          <span className="brand-name">Zexor</span>
        </div>
        <div className="row" style={{ gap: '0.5rem' }}>
          {actions}
        </div>
      </header>
      <main className="page">{children}</main>
    </div>
  );
}
