import { CheckIcon, GlobeIcon } from './Icons';
import { LANGUAGES, useI18n } from '../i18n';

/** Выбор языка интерфейса — те же языки, что в боте. Стоит в конце страницы входа и профиля. */
export default function LanguageCard() {
  const { lang, setLang, t } = useI18n();
  return (
    <section className="card">
      <span className="label">
        <GlobeIcon width={14} height={14} style={{ verticalAlign: '-2px' }} /> {t('Язык')}
      </span>
      <div className="lang-grid" role="radiogroup" aria-label={t('Язык')}>
        {LANGUAGES.map((option) => (
          <button
            key={option.code}
            type="button"
            role="radio"
            aria-checked={option.code === lang}
            className={`lang-option ${option.code === lang ? 'lang-option-active' : ''}`}
            onClick={() => setLang(option.code)}
          >
            <span>{option.name}</span>
            {option.code === lang && <CheckIcon width={16} height={16} />}
          </button>
        ))}
      </div>
    </section>
  );
}
