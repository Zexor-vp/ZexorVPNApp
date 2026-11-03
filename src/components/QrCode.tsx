import { useMemo } from 'react';
import qrcode from 'qrcode-generator';

interface Props {
  /** Что зашифровать (ссылка на бота или страницу кабинета). */
  value: string;
  /** Сторона картинки в пикселях. */
  size?: number;
}

/**
 * QR-код как встроенный SVG: рисуется локально, без запросов в сеть — ссылка со входом не уходит третьим сторонам.
 * Фон всегда белый, модули чёрные (иначе телефон плохо читает в тёмной теме).
 */
export default function QrCode({ value, size = 200 }: Props) {
  const svg = useMemo(() => {
    const qr = qrcode(0, 'M');
    qr.addData(value);
    qr.make();
    const count = qr.getModuleCount();
    const quiet = 3;
    const total = count + quiet * 2;
    let path = '';
    for (let row = 0; row < count; row += 1) {
      for (let col = 0; col < count; col += 1) {
        if (qr.isDark(row, col)) path += `M${col + quiet} ${row + quiet}h1v1h-1z`;
      }
    }
    return { total, path };
  }, [value]);

  return (
    <svg
      className="qr-code"
      width={size}
      height={size}
      viewBox={`0 0 ${svg.total} ${svg.total}`}
      shapeRendering="crispEdges"
      role="img"
      aria-label="QR"
    >
      <rect width={svg.total} height={svg.total} fill="#ffffff" />
      <path d={svg.path} fill="#000000" />
    </svg>
  );
}
