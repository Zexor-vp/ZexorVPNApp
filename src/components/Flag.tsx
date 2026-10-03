import type { FlagComponent } from 'country-flag-icons/react/3x2';
import {
  AE, AM, AT, AU, AZ, BE, BG, BR, BY, CA, CH, CN, CY, CZ, DE, DK, EE, ES, FI, FR, GB, GE, GR, HK, HR, HU, IE, IL,
  IN, IS, IT, JP, KR, KZ, LT, LU, LV, MD, NL, NO, PL, PT, RO, RS, RU, SE, SG, SI, SK, TR, UA, US,
} from 'country-flag-icons/react/3x2';

// Windows не умеет рисовать флаги-эмодзи (вместо 🇷🇺 показывает буквы «RU»), поэтому рисуем SVG.
// В пакете флаги по странам; берём только те, что реально встречаются в подписках — остальные
// остаются эмодзи, зато бандл не тащит все 250 флагов.
const FLAGS: Record<string, FlagComponent> = {
  AE, AM, AT, AU, AZ, BE, BG, BR, BY, CA, CH, CN, CY, CZ, DE, DK, EE, ES, FI, FR, GB, GE, GR, HK, HR, HU, IE, IL,
  IN, IS, IT, JP, KR, KZ, LT, LU, LV, MD, NL, NO, PL, PT, RO, RS, RU, SE, SG, SI, SK, TR, UA, US,
};

const REGIONAL_A = 0x1f1e6;
const FLAG_AT_START = /^\s*([\u{1F1E6}-\u{1F1FF}])([\u{1F1E6}-\u{1F1FF}])\s*/u;

/** Название сервера без флага-эмодзи в начале и код страны (`CZ`), если флаг был. */
export function splitFlag(remark: string): { code: string | null; label: string } {
  const match = FLAG_AT_START.exec(remark);
  if (!match) return { code: null, label: remark.trim() };
  const code = [match[1], match[2]]
    .map((char) => String.fromCharCode(65 + (char.codePointAt(0)! - REGIONAL_A)))
    .join('');
  return { code, label: remark.slice(match[0].length).trim() };
}

/** Название сервера с флагом: SVG-флаг, если он есть в наборе, иначе исходное название с эмодзи. */
export function ServerName({ remark }: { remark: string }) {
  const { code, label } = splitFlag(remark);
  const Icon = code ? FLAGS[code] : undefined;
  if (!Icon) return <>{remark.trim()}</>;
  return (
    <span className="server-name">
      <Icon className="server-flag" aria-hidden />
      {label}
    </span>
  );
}
