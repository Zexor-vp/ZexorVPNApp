import { useCallback, useEffect, useRef, useState, type FormEvent } from 'react';
import { getVersion } from '@tauri-apps/api/app';
import Button from '../components/Button';
import PageShell from '../components/PageShell';
import { ChevronDownIcon, PlusIcon } from '../components/Icons';
import { reportAuthLoss, useAsync } from '../hooks/useAsync';
import { useSupportUnread } from '../hooks/useSupportUnread';
import {
  CABINET_URL,
  SUPPORT_URL,
  TICKET_STATUS_LABEL,
  createTicket,
  getFaq,
  getProtocol,
  getSubscription,
  getTicket,
  getTickets,
  htmlToText,
  replyTicket,
  type TicketDetail,
  type TicketSummary,
} from '../lib/cabinet';
import { connectionStatus, errorMessage, openExternal } from '../lib/commands';

const THREAD_POLL_MS = 15_000;

const statusChip = (status: string) =>
  status === 'closed' ? 'chip chip-danger' : status === 'answered' ? 'chip' : 'chip chip-warn';

const formatWhen = (iso: string) =>
  new Date(iso).toLocaleString('ru-RU', { day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit' });

/** Данные, которые обычно просит поддержка: версия, подключение, протокол, подписка. Секретов нет. */
async function collectDiagnostics(): Promise<string> {
  const settle = async <T,>(fn: () => Promise<T>): Promise<T | null> => {
    try {
      return await fn();
    } catch {
      return null;
    }
  };
  const [version, status, sub, protocol] = await Promise.all([
    settle(getVersion),
    settle(connectionStatus),
    settle(getSubscription),
    settle(getProtocol),
  ]);
  const s = sub?.subscription;
  return [
    'Данные приложения Zexor VPN для Windows:',
    `Версия: ${version ?? 'неизвестно'}`,
    `Система: ${navigator.userAgent}`,
    `Подключение: ${status?.connected ? `да, ${status.node_remark ?? '?'}` : 'нет'}`,
    `Протокол: ${protocol?.active_protocol ?? 'неизвестно'}`,
    s
      ? `Подписка: ${s.tariff_name ?? 'без названия'}, осталось ${s.days_left} дн., трафик ${s.traffic_used_gb.toFixed(1)}/${s.traffic_limit_gb} ГБ`
      : 'Подписка: нет данных',
  ].join('\n');
}

type View = { kind: 'list' } | { kind: 'new' } | { kind: 'thread'; id: number };

export default function SupportPage() {
  const [view, setView] = useState<View>({ kind: 'list' });

  return (
    <PageShell
      actions={
        view.kind === 'list' ? (
          <button className="icon-btn" aria-label="Новое обращение" title="Новое обращение" onClick={() => setView({ kind: 'new' })}>
            <PlusIcon />
          </button>
        ) : undefined
      }
    >
      {view.kind === 'list' && <TicketList onNew={() => setView({ kind: 'new' })} onOpen={(id) => setView({ kind: 'thread', id })} />}
      {view.kind === 'new' && (
        <NewTicket onCancel={() => setView({ kind: 'list' })} onCreated={(id) => setView({ kind: 'thread', id })} />
      )}
      {view.kind === 'thread' && <Thread id={view.id} onBack={() => setView({ kind: 'list' })} />}
    </PageShell>
  );
}

function TicketList({ onNew, onOpen }: { onNew: () => void; onOpen: (id: number) => void }) {
  const tickets = useAsync(() => getTickets(), []);
  const { unread, total } = useSupportUnread();

  // Список обновляется сам: раз в полминуты и сразу, когда пришёл новый ответ.
  const { reload } = tickets;
  useEffect(() => {
    const timer = window.setInterval(() => void reload(), 30_000);
    return () => window.clearInterval(timer);
  }, [reload]);
  useEffect(() => {
    void reload();
  }, [total, reload]);

  const faq = useAsync(() => getFaq('ru'), []);
  const [faqOpen, setFaqOpen] = useState<number | null>(null);

  return (
    <>
      <div className="page-title">
        <h1>Поддержка</h1>
        <p>Мои обращения, ответы на частые вопросы и связь с нами</p>
      </div>

      <section className="card card-glow">
        <Button onClick={onNew}>Создать обращение</Button>
        <p className="muted" style={{ margin: 0 }}>
          Опишите проблему — ответим здесь же, в приложении, а уведомление придёт в Telegram. Нашли ошибку? Сообщите — подарим
          бесплатные дни к подписке.
        </p>
      </section>

      <section className="card">
        <span className="label">Мои обращения</span>
        {tickets.loading && !tickets.data ? (
          <div className="empty">
            <span className="spinner" aria-hidden />
          </div>
        ) : tickets.data && tickets.data.items.length > 0 ? (
          <div className="list">
            {tickets.data.items.map((ticket: TicketSummary) => (
              <button key={ticket.id} className="ticket-item" onClick={() => onOpen(ticket.id)}>
                <span className="ticket-main">
                  <strong className={unread[ticket.id] ? 'ticket-title-unread' : undefined}>{ticket.title}</strong>
                  <span className="muted ticket-preview">
                    {ticket.last_message ? `${ticket.last_message.is_from_admin ? 'Поддержка: ' : 'Вы: '}${ticket.last_message.message_text}` : '—'}
                  </span>
                </span>
                <span className="ticket-side">
                  {unread[ticket.id] ? <span className="badge-red" aria-label={`Новых ответов: ${unread[ticket.id]}`}>{unread[ticket.id]}</span> : <span className={statusChip(ticket.status)}>{TICKET_STATUS_LABEL[ticket.status] ?? ticket.status}</span>}
                  <span className="muted">{formatWhen(ticket.updated_at)}</span>
                </span>
              </button>
            ))}
          </div>
        ) : tickets.error ? (
          <>
            <p className="form-error">{tickets.error}</p>
            <Button variant="secondary" onClick={() => void openExternal(SUPPORT_URL)}>
              Написать в Telegram
            </Button>
          </>
        ) : (
          <p className="empty">Обращений пока нет.</p>
        )}
      </section>

      <section className="card">
        <span className="label">Частые вопросы</span>
        {faq.loading && !faq.data ? (
          <div className="empty">
            <span className="spinner" aria-hidden />
          </div>
        ) : faq.data && faq.data.length > 0 ? (
          <div className="list">
            {[...faq.data]
              .sort((a, b) => a.order - b.order)
              .map((page) => (
                <div key={page.id} className="faq-item">
                  <button className="faq-question" aria-expanded={faqOpen === page.id} onClick={() => setFaqOpen(faqOpen === page.id ? null : page.id)}>
                    <span>{page.title}</span>
                    <ChevronDownIcon style={{ transform: faqOpen === page.id ? 'rotate(180deg)' : undefined }} />
                  </button>
                  {faqOpen === page.id && <p className="faq-answer">{htmlToText(page.content)}</p>}
                </div>
              ))}
          </div>
        ) : (
          <p className="muted" style={{ margin: 0 }}>
            {faq.error ?? 'Пока нет статей.'}
          </p>
        )}
      </section>

      <section className="card">
        <span className="label">Другие способы связи</span>
        <Button variant="secondary" onClick={() => void openExternal(SUPPORT_URL)}>
          Написать в Telegram
        </Button>
        <Button variant="ghost" onClick={() => void openExternal(`${CABINET_URL}/support`)}>
          Открыть поддержку в кабинете
        </Button>
      </section>
    </>
  );
}

function NewTicket({ onCancel, onCreated }: { onCancel: () => void; onCreated: (id: number) => void }) {
  const [title, setTitle] = useState('');
  const [message, setMessage] = useState('');
  const [withDiagnostics, setWithDiagnostics] = useState(true);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function submit(event: FormEvent) {
    event.preventDefault();
    setSending(true);
    setError(null);
    try {
      let text = message.trim();
      if (withDiagnostics) text = `${text}\n\n— — —\n${await collectDiagnostics()}`;
      const created = await createTicket(title.trim(), text.slice(0, 4000));
      onCreated(created.id);
    } catch (err) {
      if (!reportAuthLoss(err)) setError(errorMessage(err));
    } finally {
      setSending(false);
    }
  }

  return (
    <>
      <div className="page-title">
        <h1>Новое обращение</h1>
        <p>Расскажите, что случилось, — так нам проще помочь</p>
      </div>
      <form className="card" onSubmit={submit} style={{ gap: '0.75rem' }}>
        <input className="text-input" placeholder="Коротко о проблеме" value={title} maxLength={120} onChange={(e) => setTitle(e.target.value)} />
        <textarea
          className="text-input text-area"
          placeholder="Подробности: что делали, что увидели, какая ошибка"
          rows={6}
          value={message}
          maxLength={3500}
          onChange={(e) => setMessage(e.target.value)}
        />
        <label className="check-row">
          <input type="checkbox" checked={withDiagnostics} onChange={(e) => setWithDiagnostics(e.target.checked)} />
          <span>Приложить данные приложения (версия, подключение, подписка) — без паролей и ключей</span>
        </label>
        {error && <p className="form-error">{error}</p>}
        <div className="modal-actions">
          <Button type="button" variant="ghost" onClick={onCancel}>
            Отмена
          </Button>
          <Button type="submit" loading={sending} disabled={title.trim().length < 3 || message.trim().length === 0}>
            Отправить
          </Button>
        </div>
      </form>
    </>
  );
}

function Thread({ id, onBack }: { id: number; onBack: () => void }) {
  const { markSeen, refresh } = useSupportUnread();
  const [ticket, setTicket] = useState<TicketDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [reply, setReply] = useState('');
  const [sending, setSending] = useState(false);
  const bottomRef = useRef<HTMLDivElement>(null);

  const load = useCallback(async () => {
    try {
      const detail = await getTicket(id);
      setTicket(detail);
      setError(null);
      const lastAdmin = [...detail.messages].reverse().find((m) => m.is_from_admin);
      if (lastAdmin) markSeen(id, lastAdmin.id);
    } catch (err) {
      if (!reportAuthLoss(err)) setError(errorMessage(err));
    }
  }, [id, markSeen]);

  useEffect(() => {
    void load();
    const timer = window.setInterval(() => void load(), THREAD_POLL_MS);
    return () => window.clearInterval(timer);
  }, [load]);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ block: 'end' });
  }, [ticket?.messages.length]);

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (!reply.trim()) return;
    setSending(true);
    try {
      await replyTicket(id, reply.trim());
      setReply('');
      await load();
      refresh();
    } catch (err) {
      if (!reportAuthLoss(err)) setError(errorMessage(err));
    } finally {
      setSending(false);
    }
  }

  const closed = ticket?.status === 'closed';

  return (
    <>
      <div className="page-title">
        <button className="link-btn" style={{ padding: 0 }} onClick={onBack}>
          ← Все обращения
        </button>
        <h1 style={{ marginTop: '0.4rem' }}>{ticket?.title ?? 'Обращение'}</h1>
        {ticket && <span className={statusChip(ticket.status)}>{TICKET_STATUS_LABEL[ticket.status] ?? ticket.status}</span>}
      </div>

      {error && (
        <div className="card">
          <p className="form-error">{error}</p>
        </div>
      )}

      <section className="card">
        {!ticket ? (
          <div className="empty">
            <span className="spinner" aria-hidden />
          </div>
        ) : (
          <div className="thread">
            {ticket.messages.map((message) => (
              <div key={message.id} className={`bubble ${message.is_from_admin ? 'bubble-admin' : 'bubble-user'}`}>
                <span className="bubble-author">{message.is_from_admin ? 'Поддержка' : 'Вы'}</span>
                <p>{message.message_text}</p>
                {message.has_media && <span className="muted">К сообщению приложен файл — он виден в боте и кабинете.</span>}
                <span className="bubble-time">{formatWhen(message.created_at)}</span>
              </div>
            ))}
            <div ref={bottomRef} />
          </div>
        )}
      </section>

      {ticket && !closed && !ticket.is_reply_blocked && (
        <form className="card" onSubmit={submit} style={{ gap: '0.6rem' }}>
          <textarea className="text-input text-area" rows={3} placeholder="Ваш ответ" value={reply} maxLength={3900} onChange={(e) => setReply(e.target.value)} />
          <Button type="submit" loading={sending} disabled={!reply.trim()}>
            Отправить
          </Button>
        </form>
      )}
      {ticket && (closed || ticket.is_reply_blocked) && (
        <p className="muted" style={{ margin: '0 0.25rem' }}>
          {closed ? 'Обращение закрыто. Если вопрос остался, создайте новое.' : 'Ответить в этом обращении сейчас нельзя.'}
        </p>
      )}
    </>
  );
}
