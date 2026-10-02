import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { isPermissionGranted, requestPermission, sendNotification } from '@tauri-apps/plugin-notification';
import { getTicket, getTickets, type TicketSummary } from '../lib/cabinet';

const POLL_MS = 60_000;
const STORAGE_KEY = 'zexor.support.seen';

/** id обращения → id последнего прочитанного сообщения поддержки. */
type SeenMap = Record<string, number>;

function loadSeen(): SeenMap | null {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    return raw ? (JSON.parse(raw) as SeenMap) : null;
  } catch {
    return null;
  }
}

function saveSeen(seen: SeenMap) {
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(seen));
  } catch {
    // Без хранилища уведомления просто повторятся после перезапуска.
  }
}

interface SupportUnread {
  /** Сколько новых ответов поддержки по каждому обращению (id → число). */
  unread: Record<number, number>;
  /** Всего непрочитанных ответов. */
  total: number;
  /** Отметить обращение прочитанным до сообщения `lastMessageId`. */
  markSeen: (ticketId: number, lastMessageId: number) => void;
  /** Принудительно перечитать (после своего ответа или создания обращения). */
  refresh: () => void;
}

const Ctx = createContext<SupportUnread>({ unread: {}, total: 0, markSeen: () => undefined, refresh: () => undefined });

export const useSupportUnread = () => useContext(Ctx);

async function notify(title: string, body: string) {
  try {
    let granted = await isPermissionGranted();
    if (!granted) granted = (await requestPermission()) === 'granted';
    if (granted) sendNotification({ title, body });
  } catch {
    // Уведомления не критичны: красный значок в приложении всё равно появится.
  }
}

/**
 * Раз в минуту проверяет обращения. Новый ответ поддержки → системное уведомление Windows
 * (даже если окно спрятано в трей) и красные значки в приложении.
 */
export function SupportUnreadProvider({ enabled, children }: { enabled: boolean; children: ReactNode }) {
  const [unread, setUnread] = useState<Record<number, number>>({});
  const seenRef = useRef<SeenMap | null>(loadSeen());
  const notifiedRef = useRef<Set<string>>(new Set());
  const firstRunRef = useRef(true);
  const [tick, setTick] = useState(0);

  const poll = useCallback(async () => {
    let items: TicketSummary[];
    try {
      items = (await getTickets()).items;
    } catch {
      return; // нет сети или поддержка выключена — попробуем в следующий раз
    }

    // Первый запуск: всё, что уже есть, считаем прочитанным, чтобы не засыпать уведомлениями.
    if (seenRef.current === null) {
      const initial: SeenMap = {};
      for (const ticket of items) if (ticket.last_message) initial[ticket.id] = ticket.last_message.id;
      seenRef.current = initial;
      saveSeen(initial);
    }
    const seen = seenRef.current;

    const next: Record<number, number> = {};
    for (const ticket of items) {
      const last = ticket.last_message;
      if (!last || !last.is_from_admin || (seen[ticket.id] ?? 0) >= last.id) continue;
      // Сколько ответов поддержки пришло после прочитанного: нужна переписка, в списке только последнее сообщение.
      let count = 1;
      try {
        const detail = await getTicket(ticket.id);
        count = Math.max(1, detail.messages.filter((m) => m.is_from_admin && m.id > (seen[ticket.id] ?? 0)).length);
      } catch {
        // остаёмся на «1»
      }
      next[ticket.id] = count;

      const key = `${ticket.id}:${last.id}`;
      if (!notifiedRef.current.has(key)) {
        notifiedRef.current.add(key);
        if (!firstRunRef.current) void notify('Ответ поддержки', ticket.title);
      }
    }
    firstRunRef.current = false;
    setUnread(next);
  }, []);

  useEffect(() => {
    if (!enabled) {
      setUnread({});
      return;
    }
    void poll();
    const timer = window.setInterval(() => void poll(), POLL_MS);
    return () => window.clearInterval(timer);
  }, [enabled, poll, tick]);

  const markSeen = useCallback((ticketId: number, lastMessageId: number) => {
    const seen = seenRef.current ?? {};
    if ((seen[ticketId] ?? 0) >= lastMessageId) return;
    seenRef.current = { ...seen, [ticketId]: lastMessageId };
    saveSeen(seenRef.current);
    setUnread((prev) => {
      if (!(ticketId in prev)) return prev;
      const { [ticketId]: _removed, ...rest } = prev;
      return rest;
    });
  }, []);

  const value = useMemo<SupportUnread>(
    () => ({
      unread,
      total: Object.values(unread).reduce((sum, n) => sum + n, 0),
      markSeen,
      refresh: () => setTick((t) => t + 1),
    }),
    [unread, markSeen],
  );

  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}
