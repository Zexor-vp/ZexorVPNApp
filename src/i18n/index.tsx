// Языки приложения — те же, что в боте: ru, en, de, fr, es, it, pl, nl, pt, fa.
//
// Ключ перевода — русский текст как он написан в коде (как в gettext): переводы лежат в locales/<код>.ts,
// а у русского словаря нет — он и есть исходник. Нет перевода → показывается русский текст, поэтому
// приложение не ломается, если какая-то строка не переведена.
//
// Параметры подставляются как `{имя}`: t('Осталось {n} дн.', { n: 5 }).
import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from 'react';
import de from './locales/de';
import en from './locales/en';
import es from './locales/es';
import fa from './locales/fa';
import fr from './locales/fr';
import it from './locales/it';
import nl from './locales/nl';
import pl from './locales/pl';
import pt from './locales/pt';

export const LANGUAGES = [
  { code: 'ru', name: 'Русский', locale: 'ru-RU' },
  { code: 'en', name: 'English', locale: 'en-GB' },
  { code: 'de', name: 'Deutsch', locale: 'de-DE' },
  { code: 'fr', name: 'Français', locale: 'fr-FR' },
  { code: 'es', name: 'Español', locale: 'es-ES' },
  { code: 'it', name: 'Italiano', locale: 'it-IT' },
  { code: 'pl', name: 'Polski', locale: 'pl-PL' },
  { code: 'nl', name: 'Nederlands', locale: 'nl-NL' },
  { code: 'pt', name: 'Português', locale: 'pt-PT' },
  { code: 'fa', name: 'فارسی', locale: 'fa-IR' },
] as const;

export type LangCode = (typeof LANGUAGES)[number]['code'];

type Dict = Record<string, string>;
const DICTS: Record<Exclude<LangCode, 'ru'>, Dict> = { en, de, fr, es, it, pl, nl, pt, fa };

const STORAGE_KEY = 'zexor.lang';
const isLang = (value: string | null | undefined): value is LangCode => LANGUAGES.some((l) => l.code === value);

/** Сохранённый выбор → язык системы (если он среди наших) → английский, как в боте по умолчанию. */
function detectLanguage(): LangCode {
  try {
    const saved = window.localStorage.getItem(STORAGE_KEY);
    if (isLang(saved)) return saved;
  } catch {
    // без хранилища просто берём язык системы
  }
  const system = (typeof navigator !== 'undefined' ? navigator.language : 'en').slice(0, 2).toLowerCase();
  return isLang(system) ? system : 'en';
}

let currentLang: LangCode = detectLanguage();

const PLACEHOLDER = /\{(\w+)\}/g;

function fill(template: string, params?: Record<string, string | number>): string {
  if (!params) return template;
  return template.replace(PLACEHOLDER, (whole, name: string) => (name in params ? String(params[name]) : whole));
}

// Ключи с подстановками (`{0}`, `{status}`…) превращаются в регулярки — так переводятся и сообщения
// Rust-части приложения, где число и имя подставлены уже в готовый русский текст.
const patternCache = new Map<string, { entries: { key: string; regex: RegExp; names: string[] }[] }>();

function patternsFor(lang: Exclude<LangCode, 'ru'>) {
  const cached = patternCache.get(lang);
  if (cached) return cached;
  const entries: { key: string; regex: RegExp; names: string[] }[] = [];
  for (const key of Object.keys(DICTS[lang])) {
    if (!PLACEHOLDER.test(key)) continue;
    PLACEHOLDER.lastIndex = 0;
    const names: string[] = [];
    const source = key
      .split(PLACEHOLDER)
      .map((part, index) => {
        if (index % 2 === 1) {
          names.push(part);
          return '(.+?)';
        }
        return part.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
      })
      .join('');
    entries.push({ key, regex: new RegExp(`^${source}$`, 's'), names });
  }
  const result = { entries };
  patternCache.set(lang, result);
  return result;
}

/** Перевод текста (ключ — русский оригинал) на язык `lang`. */
export function translate(lang: LangCode, ru: string, params?: Record<string, string | number>): string {
  if (lang === 'ru') return fill(ru, params);
  const dict = DICTS[lang];
  const exact = dict[ru];
  if (exact !== undefined) return fill(exact, params);
  // Готовый русский текст с уже подставленными значениями (из Rust) — ищем по шаблону.
  for (const { key, regex, names } of patternsFor(lang).entries) {
    const match = regex.exec(ru);
    if (!match) continue;
    const found: Record<string, string> = {};
    names.forEach((name, index) => {
      // Вложенный текст (например, причина ошибки внутри сообщения) тоже переводим.
      found[name] = translate(lang, match[index + 1]);
    });
    return fill(dict[key], { ...found, ...params });
  }
  return fill(ru, params);
}

/** Перевод вне React-компонентов (сообщения об ошибках и т. п.) — по текущему языку. */
export const tr = (ru: string, params?: Record<string, string | number>): string => translate(currentLang, ru, params);

export const currentLocale = (): string => LANGUAGES.find((l) => l.code === currentLang)?.locale ?? 'en-GB';

interface I18n {
  lang: LangCode;
  setLang: (lang: LangCode) => void;
  t: (ru: string, params?: Record<string, string | number>) => string;
}

const Ctx = createContext<I18n>({ lang: currentLang, setLang: () => undefined, t: (ru, params) => fill(ru, params) });

export function I18nProvider({ children }: { children: ReactNode }) {
  const [lang, setLangState] = useState<LangCode>(currentLang);

  const setLang = useCallback((next: LangCode) => {
    currentLang = next;
    setLangState(next);
    try {
      window.localStorage.setItem(STORAGE_KEY, next);
    } catch {
      // выбор просто не запомнится
    }
  }, []);

  // Язык документа и направление текста (фарси — справа налево).
  useEffect(() => {
    document.documentElement.lang = lang;
    document.documentElement.dir = lang === 'fa' ? 'rtl' : 'ltr';
  }, [lang]);

  const value = useMemo<I18n>(
    () => ({ lang, setLang, t: (ru, params) => translate(lang, ru, params) }),
    [lang, setLang],
  );
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

export const useI18n = () => useContext(Ctx);
export const useT = () => useContext(Ctx).t;
