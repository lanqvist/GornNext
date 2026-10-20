/**
 * Честная статистика — главное отличие Горна от остальных трекеров жанра.
 *
 * Все известные трекеры (TITrack, Fenix, Exile Diary, Exilence, PoeStash,
 * Wealthy Exile, dIVa) показывают СРЕДНЕЕ значение дохода в час.
 * Для дропа в ARPG это плохая оценка: распределение с тяжёлым хвостом,
 * одна редкая дорогая вещь задирает среднее так, что оно перестаёт
 * описывать типичный забег. Игрок принимает решения по числу,
 * которого не увидит ни в одном забеге.
 *
 * Поэтому главная метрика — медиана, а рядом всегда показывается,
 * насколько ей можно верить.
 */

export interface RateSummary {
  /** Медиана — главное число на экране. */
  median: number;
  /** Среднее — рядом мелким шрифтом, для сравнения с другими трекерами. */
  mean: number;
  p25: number;
  p75: number;
  /** Сколько забегов легло в расчёт. */
  sampleSize: number;
  confidence: Confidence;
  /** Доля всей добычи (сумма положительных чистых ФЕ забегов), принесённая
   *  самым удачным забегом, в [0, 1]. Если она велика — цифра держится
   *  на одной находке. */
  topShare: number;
}

export type Confidence = 'insufficient' | 'emerging' | 'solid';

/** Пороги подобраны так, чтобы бейдж менялся на осмысленных отрезках фарма. */
export const CONFIDENCE_THRESHOLDS = { emerging: 15, solid: 50 } as const;

export function confidenceOf(sampleSize: number): Confidence {
  if (sampleSize < CONFIDENCE_THRESHOLDS.emerging) return 'insufficient';
  if (sampleSize < CONFIDENCE_THRESHOLDS.solid) return 'emerging';
  return 'solid';
}

/** Линейно интерполированный перцентиль. */
export function percentile(sorted: number[], p: number): number {
  if (sorted.length === 0) return 0;
  if (sorted.length === 1) return sorted[0]!;
  const idx = (sorted.length - 1) * p;
  const lo = Math.floor(idx);
  const hi = Math.ceil(idx);
  if (lo === hi) return sorted[lo]!;
  return sorted[lo]! + (sorted[hi]! - sorted[lo]!) * (idx - lo);
}

export function median(values: number[]): number {
  return percentile([...values].sort((a, b) => a - b), 0.5);
}

/**
 * Доля лучшего в сумме положительных значений, в [0, 1].
 *
 * Убыточные забеги в знаменатель не идут: «вся добыча» — это то, что
 * принесли, а не итог с вычетом трат. С ними знаменатель проседал ниже
 * лучшего забега, и выходило «один забег дал 120% всей добычи».
 */
export function topShareOf(amounts: number[]): number {
  let total = 0;
  let max = 0;
  for (const v of amounts) {
    if (!(v > 0)) continue;
    total += v;
    if (v > max) max = v;
  }
  return total > 0 ? Math.min(1, max / total) : 0;
}

/**
 * Сводка по набору значений (например, ФЕ/час по каждому забегу).
 * amounts — чистые ФЕ тех же забегов: доля лучшего считается по добыче,
 * а не по темпам (короткий забег с высоким темпом — не «половина добычи»).
 * Без них доля считается по самим значениям.
 */
export function summarize(values: number[], amounts: number[] = values): RateSummary {
  const n = values.length;
  if (n === 0) {
    return {
      median: 0,
      mean: 0,
      p25: 0,
      p75: 0,
      sampleSize: 0,
      confidence: 'insufficient',
      topShare: 0,
    };
  }
  const sorted = [...values].sort((a, b) => a - b);
  const total = values.reduce((a, b) => a + b, 0);

  return {
    median: percentile(sorted, 0.5),
    mean: total / n,
    p25: percentile(sorted, 0.25),
    p75: percentile(sorted, 0.75),
    sampleSize: n,
    confidence: confidenceOf(n),
    topShare: topShareOf(amounts),
  };
}

/** Доход в час из количества и длительности. */
export function perHour(amount: number, durationMs: number): number {
  if (durationMs <= 0) return 0;
  return (amount / durationMs) * 3_600_000;
}
