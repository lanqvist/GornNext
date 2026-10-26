import { tr } from '../../core/i18n.js';

/**
 * Жар темпа: темп карты к норме (в час за последние 20 забегов) — цветом
 * каления стали, как кузнец читает температуру по металлу. Норма — вишнёвый,
 * выше — оранжевый, жёлтый, белый; ниже — тёмно-вишнёвый и зола.
 */

/** Шкала каления: доля 0…1 → цвет. */
const STOPS: Array<[number, [number, number, number]]> = [
  [0, [0x4d, 0x47, 0x44]], // зола
  [0.2, [0x6e, 0x1a, 0x10]], // тёмно-вишнёвый
  [0.38, [0xb3, 0x26, 0x1e]], // вишнёвый — норма
  [0.55, [0xff, 0x7a, 0x1a]], // оранжевый
  [0.75, [0xff, 0xc9, 0x3c]], // жёлтый
  [1, [0xff, 0xf3, 0xd6]], // белый
];

/** Отклонение темпа, %, на концах шкалы: −50 — зола, +80 — белый. */
const LOW = -50;
const HIGH = 80;

export interface Heat {
  /** Цвет каления, `rgb(…)`. */
  color: string;
  /** Сила свечения кромки 0,15…1. */
  glow: number;
  /** Название каления — для подсказки у процента. */
  name: string;
  /**
   * Искры: 0 — нет (норма и ниже), 1 — редкие, 2 — чаще, 3 — белый жар.
   * Ниже нормы искр нет совсем: остывший горн не должен дёргать глаз.
   */
  sparks: 0 | 1 | 2 | 3;
}

function colorAt(t: number): string {
  const k = Math.max(0, Math.min(1, t));
  for (let i = 1; i < STOPS.length; i++) {
    const [t1, c1] = STOPS[i]!;
    if (k <= t1) {
      const [t0, c0] = STOPS[i - 1]!;
      const f = (k - t0) / (t1 - t0);
      return `rgb(${c0.map((v, j) => Math.round(v + (c1[j]! - v) * f)).join(', ')})`;
    }
  }
  return 'rgb(255, 243, 214)';
}

export function heatOf(deltaPct: number): Heat {
  const t = (deltaPct - LOW) / (HIGH - LOW);
  const glow = Math.max(0.15, Math.min(1, (t - 0.1) / 0.9));
  const name =
    deltaPct < -30
      ? tr('зола', 'ash')
      : deltaPct < -10
        ? tr('тёмно-вишнёвый жар', 'dark cherry heat')
        : deltaPct < 10
          ? tr('вишнёвый жар', 'cherry heat')
          : deltaPct < 30
            ? tr('оранжевый жар', 'orange heat')
            : deltaPct < 60
              ? tr('жёлтый жар', 'yellow heat')
              : tr('белый жар', 'white heat');
  const sparks = deltaPct < 10 ? 0 : deltaPct < 30 ? 1 : deltaPct < 60 ? 2 : 3;
  return { color: colorAt(t), glow, name, sparks };
}
