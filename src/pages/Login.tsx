import { useState, type FormEvent } from 'react';
import Button from '../components/Button';
import { errorMessage, login, type SessionInfo } from '../lib/commands';

interface Props {
  onSuccess: (session: SessionInfo) => void;
}

export default function Login({ onSuccess }: Props) {
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    setError(null);
    setLoading(true);
    try {
      const session = await login(email.trim(), password);
      onSuccess(session);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setLoading(false);
    }
  }

  return (
    <main className="app-shell login-shell">
      <div className="brand">
        <span className="brand-dot" />
        <h1>Zexor VPN</h1>
      </div>

      <form className="card login-card" onSubmit={handleSubmit}>
        <label className="field">
          <span>Email</span>
          <input
            type="email"
            required
            autoFocus
            autoComplete="email"
            value={email}
            onChange={(e) => setEmail(e.target.value)}
            placeholder="you@example.com"
          />
        </label>

        <label className="field">
          <span>Пароль</span>
          <input
            type="password"
            required
            autoComplete="current-password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            placeholder="••••••••"
          />
        </label>

        {error && <p className="form-error">{error}</p>}

        <Button type="submit" loading={loading} className="login-submit">
          Войти
        </Button>
      </form>

      <p className="login-hint">Аккаунт создаётся в Telegram-боте или в веб-кабинете.</p>
    </main>
  );
}
