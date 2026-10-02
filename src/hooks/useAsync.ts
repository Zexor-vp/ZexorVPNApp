import { useCallback, useEffect, useRef, useState } from 'react';
import { errorMessage, needsLogin } from '../lib/commands';

/** Контекст «сессия потеряна»: любая страница сообщает сюда, если сервер отказал по токену. */
let authLostHandler: (() => void) | null = null;
export const setAuthLostHandler = (handler: (() => void) | null) => {
  authLostHandler = handler;
};
export const reportAuthLoss = (err: unknown): boolean => {
  if (needsLogin(err)) {
    authLostHandler?.();
    return true;
  }
  return false;
};

interface State<T> {
  data: T | null;
  error: string | null;
  loading: boolean;
}

/** Грузит данные при монтировании и по `reload()`. Ошибка — строкой для интерфейса. */
export function useAsync<T>(load: () => Promise<T>, deps: unknown[] = []) {
  const [state, setState] = useState<State<T>>({ data: null, error: null, loading: true });
  const seq = useRef(0);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const loader = useCallback(load, deps);

  const reload = useCallback(async () => {
    const mine = ++seq.current;
    setState((prev) => ({ ...prev, loading: true, error: null }));
    try {
      const data = await loader();
      if (mine === seq.current) setState({ data, error: null, loading: false });
    } catch (err) {
      if (mine !== seq.current) return;
      if (reportAuthLoss(err)) return;
      setState((prev) => ({ data: prev.data, error: errorMessage(err), loading: false }));
    }
  }, [loader]);

  useEffect(() => {
    void reload();
    return () => {
      seq.current++;
    };
  }, [reload]);

  return { ...state, reload };
}
