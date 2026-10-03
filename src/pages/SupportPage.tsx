import { useCallback, useEffect, useRef, useState, type ChangeEvent, type FormEvent } from 'react';
import { getVersion } from '@tauri-apps/api/app';
import Button from '../components/Button';
import PageShell from '../components/PageShell';
import { ChevronDownIcon, PlusIcon } from '../components/Icons';
import { reportAuthLoss, useAsync } from '../hooks/useAsync';
import { onSupportTickets, useSupportUnread } from '../hooks/useSupportUnread';
import { currentLocale, useI18n, useT } from '../i18n';
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
  ticketPhotoUrl,
  uploadSupportPhoto,
  type TicketDetail,
  type TicketSummary,
} from '../lib/cabinet';
import { connectionStatus, errorMessage, openExternal } from '../lib/commands';
import { preparePhoto, type PreparedPhoto } from '../lib/image';
import { isMobile } from '../lib/platform';

const THREAD_POLL_MS = 5_000;

const statusChip = (status: string) =>
  status === 'closed' ? 'chip chip-danger' : status === 'answered' ? 'chip' : 'chip chip-warn';

const formatWhen = (iso: string) =>
  new Date(iso).toLocaleString(currentLocale(), { day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit' });

/** Данные, которые обычно просит поддержка: версия, подключение, протокол, подписка. Секретов нет.
 * Текст намеренно не переводится: его читает поддержка, а не пользователь. */
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
    `Данные приложения Zexor VPN для ${isMobile ? 'Android' : 'Windows'}:`,
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
  const t = useT();
  const [view, setView] = useState<View>({ kind: 'list' });

  return (
    <PageShell
      actions={
        view.kind === 'list' ? (
          <button
            className="icon-btn"
            aria-label={t('Новое обращение')}
            title={t('Новое обращение')}
            onClick={() => setView({ kind: 'new' })}
          >
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
  const t = useT();
  const { lang } = useI18n();
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

  const faq = useAsync(() => getFaq(lang), [lang]);
  const [faqOpen, setFaqOpen] = useState<number | null>(null);

  return (
    <>
      <div className="page-title">
        <h1>{t('Поддержка')}</h1>
        <p>{t('Мои обращения, ответы на частые вопросы и связь с нами')}</p>
      </div>

      <section className="card card-glow">
        <Button onClick={onNew}>{t('Создать обращение')}</Button>
        <p className="muted" style={{ margin: 0 }}>
          {t('Опишите проблему — ответим здесь же, в приложении, а уведомление придёт в Telegram. Нашли ошибку? Сообщите — подарим бесплатные дни к подписке.')}
        </p>
      </section>

      <section className="card">
        <span className="label">{t('Мои обращения')}</span>
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
                    {ticket.last_message
                      ? `${ticket.last_message.is_from_admin ? t('Поддержка') : t('Вы')}: ${ticket.last_message.message_text}`
                      : '—'}
                  </span>
                </span>
                <span className="ticket-side">
                  {unread[ticket.id] ? <span className="badge-red" aria-label={t('Новых ответов: {n}', { n: unread[ticket.id] })}>{unread[ticket.id]}</span> : <span className={statusChip(ticket.status)}>{TICKET_STATUS_LABEL[ticket.status] ? t(TICKET_STATUS_LABEL[ticket.status]) : ticket.status}</span>}
                  <span className="muted">{formatWhen(ticket.updated_at)}</span>
                </span>
              </button>
            ))}
          </div>
        ) : tickets.error ? (
          <>
            <p className="form-error">{tickets.error}</p>
            <Button variant="secondary" onClick={() => void openExternal(SUPPORT_URL)}>
              {t('Написать в Telegram')}
            </Button>
          </>
        ) : (
          <p className="empty">{t('Обращений пока нет.')}</p>
        )}
      </section>

      <section className="card">
        <span className="label">{t('Частые вопросы')}</span>
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
            {faq.error ?? t('Пока нет статей.')}
          </p>
        )}
      </section>

      <section className="card">
        <span className="label">{t('Другие способы связи')}</span>
        <Button variant="secondary" onClick={() => void openExternal(SUPPORT_URL)}>
          {t('Написать в Telegram')}
        </Button>
        <Button variant="ghost" onClick={() => void openExternal(`${CABINET_URL}/support`)}>
          {t('Открыть поддержку в кабинете')}
        </Button>
      </section>
    </>
  );
}

/** Выбор фото для сообщения: превью, кнопка «убрать» и сама загрузка при отправке. */
function usePhotoAttachment() {
  const t = useT();
  const [photo, setPhoto] = useState<PreparedPhoto | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function pick(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0];
    event.target.value = '';
    if (!file) return;
    setError(null);
    try {
      setPhoto(await preparePhoto(file));
    } catch (err) {
      setError(t(errorMessage(err)));
    }
  }

  /** `file_id` загруженного фото или `null`, если ничего не приложено. */
  async function upload(): Promise<string | null> {
    return photo ? uploadSupportPhoto(photo.mime, photo.base64) : null;
  }

  return { photo, error, pick, clear: () => setPhoto(null), upload };
}

function PhotoButton({ onPick, disabled }: { onPick: (event: ChangeEvent<HTMLInputElement>) => void; disabled?: boolean }) {
  const t = useT();
  return (
    <label className={`icon-btn photo-btn${disabled ? ' is-disabled' : ''}`} title={t('Прикрепить фото')} aria-label={t('Прикрепить фото')}>
      <input type="file" accept="image/*" onChange={onPick} disabled={disabled} hidden />
      <span aria-hidden>📎</span>
    </label>
  );
}

function PhotoPreview({ photo, onClear }: { photo: PreparedPhoto; onClear: () => void }) {
  const t = useT();
  return (
    <div className="photo-preview">
      <img src={photo.previewUrl} alt="" />
      <button type="button" className="link-btn" onClick={onClear}>
        {t('Убрать фото')}
      </button>
    </div>
  );
}

function NewTicket({ onCancel, onCreated }: { onCancel: () => void; onCreated: (id: number) => void }) {
  const t = useT();
  const [title, setTitle] = useState('');
  const [message, setMessage] = useState('');
  const [withDiagnostics, setWithDiagnostics] = useState(true);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const attachment = usePhotoAttachment();

  async function submit(event: FormEvent) {
    event.preventDefault();
    setSending(true);
    setError(null);
    try {
      let text = message.trim();
      if (withDiagnostics) text = `${text}\n\n— — —\n${await collectDiagnostics()}`;
      const created = await createTicket(title.trim(), text.slice(0, 4000), await attachment.upload());
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
        <h1>{t('Новое обращение')}</h1>
        <p>{t('Расскажите, что случилось, — так нам проще помочь')}</p>
      </div>
      <form className="card" onSubmit={submit} style={{ gap: '0.75rem' }}>
        <input className="text-input" placeholder={t('Коротко о проблеме')} value={title} maxLength={120} onChange={(e) => setTitle(e.target.value)} />
        <textarea
          className="text-input text-area"
          placeholder={t('Подробности: что делали, что увидели, какая ошибка')}
          rows={6}
          value={message}
          maxLength={3500}
          onChange={(e) => setMessage(e.target.value)}
        />
        <label className="check-row">
          <input type="checkbox" checked={withDiagnostics} onChange={(e) => setWithDiagnostics(e.target.checked)} />
          <span>{t('Приложить данные приложения (версия, подключение, подписка) — без паролей и ключей')}</span>
        </label>
        {attachment.photo && <PhotoPreview photo={attachment.photo} onClear={attachment.clear} />}
        {(error ?? attachment.error) && <p className="form-error">{error ?? attachment.error}</p>}
        <div className="modal-actions">
          <PhotoButton onPick={attachment.pick} disabled={sending} />
          <Button type="button" variant="ghost" onClick={onCancel}>
            {t('Отмена')}
          </Button>
          <Button type="submit" loading={sending} disabled={title.trim().length < 3 || message.trim().length === 0}>
            {t('Отправить')}
          </Button>
        </div>
      </form>
    </>
  );
}

function Thread({ id, onBack }: { id: number; onBack: () => void }) {
  const t = useT();
  const { markSeen, refresh } = useSupportUnread();
  const [ticket, setTicket] = useState<TicketDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [reply, setReply] = useState('');
  const [sending, setSending] = useState(false);
  const attachment = usePhotoAttachment();
  const bottomRef = useRef<HTMLDivElement>(null);
  // Лента листается как в мессенджерах: к новому сообщению едем, только если пользователь и так внизу.
  const stickToBottom = useRef(true);
  const firstScroll = useRef(true);
  const forceScroll = useRef(false);

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

  // Rust опрашивает сервер сам: как только в списке появилось более новое сообщение этого обращения,
  // перечитываем переписку сразу, не дожидаясь своего таймера.
  const lastLoadedId = useRef(0);
  useEffect(() => {
    lastLoadedId.current = ticket?.messages[ticket.messages.length - 1]?.id ?? 0;
  }, [ticket]);
  useEffect(
    () =>
      onSupportTickets((items) => {
        const mine = items.find((item) => item.id === id);
        if (mine?.last_message && mine.last_message.id > lastLoadedId.current) void load();
      }),
    [id, load],
  );

  useEffect(() => {
    const page = document.querySelector<HTMLElement>('.page');
    if (!page) return;
    const onScroll = () => {
      stickToBottom.current = page.scrollHeight - page.scrollTop - page.clientHeight < 160;
    };
    page.addEventListener('scroll', onScroll, { passive: true });
    return () => page.removeEventListener('scroll', onScroll);
  }, []);

  const scrollToBottom = useCallback((smooth: boolean) => {
    const page = document.querySelector<HTMLElement>('.page');
    page?.scrollTo({ top: page.scrollHeight, behavior: smooth ? 'smooth' : 'auto' });
  }, []);

  const messageCount = ticket?.messages.length ?? 0;
  useEffect(() => {
    if (messageCount === 0) return;
    if (firstScroll.current) {
      firstScroll.current = false;
      scrollToBottom(false);
    } else if (forceScroll.current || stickToBottom.current) {
      forceScroll.current = false;
      scrollToBottom(true);
    }
  }, [messageCount, scrollToBottom]);

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (!reply.trim() && !attachment.photo) return;
    setSending(true);
    try {
      const photoId = await attachment.upload();
      await replyTicket(id, reply.trim(), photoId);
      setReply('');
      attachment.clear();
      forceScroll.current = true;
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
          {t('← Все обращения')}
        </button>
        <h1 style={{ marginTop: '0.4rem' }}>{ticket?.title ?? t('Обращение')}</h1>
        {ticket && (
          <span className={statusChip(ticket.status)}>
            {TICKET_STATUS_LABEL[ticket.status] ? t(TICKET_STATUS_LABEL[ticket.status]) : ticket.status}
          </span>
        )}
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
                <span className="bubble-author">{message.is_from_admin ? t('Поддержка') : t('Вы')}</span>
                <p>{message.message_text}</p>
                {ticketPhotoUrl(message) ? (
                  <img
                    className="bubble-photo"
                    src={ticketPhotoUrl(message) ?? ''}
                    alt=""
                    loading="lazy"
                    onLoad={() => stickToBottom.current && scrollToBottom(false)}
                    onClick={() => void openExternal(ticketPhotoUrl(message) ?? '').catch(() => undefined)}
                  />
                ) : (
                  message.has_media && <span className="muted">{t('К сообщению приложен файл — он виден в боте и кабинете.')}</span>
                )}
                <span className="bubble-time">{formatWhen(message.created_at)}</span>
              </div>
            ))}
            <div ref={bottomRef} />
          </div>
        )}
      </section>

      {ticket && !closed && !ticket.is_reply_blocked && (
        <form className="card composer" onSubmit={submit}>
          {attachment.photo && <PhotoPreview photo={attachment.photo} onClear={attachment.clear} />}
          {attachment.error && <p className="form-error">{attachment.error}</p>}
          <div className="composer-row">
            <PhotoButton onPick={attachment.pick} disabled={sending} />
            <textarea
              className="text-input text-area"
              rows={1}
              placeholder={t('Ваш ответ')}
              value={reply}
              maxLength={3900}
              onChange={(e) => setReply(e.target.value)}
            />
            <Button type="submit" loading={sending} disabled={!reply.trim() && !attachment.photo}>
              {t('Отправить')}
            </Button>
          </div>
        </form>
      )}
      {ticket && (closed || ticket.is_reply_blocked) && (
        <p className="muted" style={{ margin: '0 0.25rem' }}>
          {closed ? t('Обращение закрыто. Если вопрос остался, создайте новое.') : t('Ответить в этом обращении сейчас нельзя.')}
        </p>
      )}
    </>
  );
}
