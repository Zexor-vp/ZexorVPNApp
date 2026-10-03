import { useCallback, useRef, useState } from 'react';
import { CheckIcon, GlobeIcon } from './Icons';
import { useOutsideClose } from '../hooks/useOutsideClose';
import { LANGUAGES, useI18n } from '../i18n';

/** Выбор языка интерфейса — те же языки, что в боте. Работает и без входа в аккаунт. */
export default function LanguageMenu() {
  const { lang, setLang, t } = useI18n();
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const close = useCallback(() => setOpen(false), []);
  useOutsideClose(ref, open, close);

  return (
    <div className="dropdown dropdown-compact" ref={ref}>
      <button
        type="button"
        className="pill-btn"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={t('Язык')}
        title={t('Язык')}
        onClick={() => setOpen((value) => !value)}
      >
        <GlobeIcon width={16} height={16} /> {lang.toUpperCase()}
      </button>
      {open && (
        <div className="dropdown-panel dropdown-panel-compact" role="listbox" style={{ width: 'min(220px, 70vw)' }}>
          {LANGUAGES.map((option) => (
            <button
              key={option.code}
              type="button"
              role="option"
              aria-selected={option.code === lang}
              className={`dropdown-item ${option.code === lang ? 'dropdown-item-active' : ''}`}
              onClick={() => {
                setOpen(false);
                setLang(option.code);
              }}
            >
              <span className="dropdown-item-main">
                <strong>{option.name}</strong>
              </span>
              {option.code === lang && <CheckIcon />}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
