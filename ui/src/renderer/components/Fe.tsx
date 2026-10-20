import { useState } from 'react';

import { tr } from '../../core/i18n.js';
import feIcon from '../assets/fe.webp';
import { num } from '../format.js';

/**
 * Число в ФЕ вместе с иконкой валюты.
 *
 * Иконка масштабируется от размера шрифта (1em), поэтому один и тот же
 * компонент одинаково хорошо выглядит и в заголовке 48px, и в строке
 * таблицы 13px — без отдельных вариантов на каждый размер.
 *
 * Иконка лежит в репозитории, а не тянется из CDN: приложение должно
 * работать без интернета и не зависеть от чужого хостинга.
 */

interface FeProps {
  value: number;
  /** Знак «+» для положительных значений — для дельт. */
  signed?: boolean;
  fractionDigits?: number;
  className?: string;
}

/**
 * Разрядность по величине.
 *
 * Многие материалы стоят доли ФЕ. Округление до целого превращало их
 * в «0 ФЕ», и предмет выглядел бесплатным, хотя цена у него есть —
 * просто маленькая. Крупные суммы дробей не требуют.
 */
function digitsFor(value: number): number {
  const abs = Math.abs(value);
  if (abs === 0 || abs >= 10) return 0;
  if (abs >= 1) return 1;
  return 2;
}

export function Fe({ value, signed = false, fractionDigits, className }: FeProps) {
  const digits = fractionDigits ?? digitsFor(value);
  const rounded = digits === 0 ? Math.round(value) : value;
  const sign = signed && rounded > 0 ? '+' : '';
  const text =
    rounded < 0 ? `−${num(Math.abs(rounded), digits)}` : num(rounded, digits);

  return (
    <span className={`fe ${className ?? ''}`}>
      {sign}
      {text}
      <FeIcon />
    </span>
  );
}

/** Иконка отдельно — для заголовков колонок и подписей без числа. */
export function FeIcon() {
  // Фолбэк через состояние, а не подмену DOM-узла: React владеет своим
  // деревом, и replaceWith ломал ему removeChild при следующем анмаунте —
  // одна несуществующая иконка обрушивала весь интерфейс в белый экран.
  const [failed, setFailed] = useState(false);

  if (failed) {
    // Искра вместо голого текста «ФЕ».
    return (
      <svg viewBox="0 0 24 24" className="fe-icon" aria-label={tr('ФЕ', 'FE')}>
        <path
          d="M12 2c1.2 3.6 4.8 5.2 4.8 9.2A4.8 4.8 0 0 1 12 16a4.8 4.8 0 0 1-4.8-4.8C7.2 7.2 10.8 5.6 12 2Z"
          fill="currentColor"
        />
      </svg>
    );
  }

  return (
    <img
      className="fe-icon"
      src={feIcon}
      alt={tr('ФЕ', 'FE')}
      title={tr('Элементиевое пламя (Flame Elementium)', 'Flame Elementium')}
      draggable={false}
      onError={() => setFailed(true)}
    />
  );
}
