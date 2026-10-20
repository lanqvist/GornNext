import { tr, uiLocale } from '../core/i18n.js';

/**
 * Неразрывный пробел — escape-последовательностью, а не символом:
 * в редакторе он неотличим от обычного, и подмена ломала числа
 * вроде «12 345» и «3 ч» переносом строки.
 */
export const NBSP = '\u00a0';

export const num = (v: number, digits = 0) =>
  new Intl.NumberFormat(uiLocale(), {
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  })
    .format(v)
    .replace(/\s/g, NBSP);

export function duration(ms: number): string {
  const total = Math.round(ms / 1000);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  // По-английски единица пишется слитно: «41 m» читается как метры.
  const [hu, mu, su] = [tr(`${NBSP}ч`, 'h'), tr(`${NBSP}м`, 'm'), tr(`${NBSP}с`, 's')];
  if (h > 0) return m > 0 ? `${h}${hu} ${m}${mu}` : `${h}${hu}`;
  if (m > 0) return s > 0 ? `${m}${mu} ${s}${su}` : `${m}${mu}`;
  return `${s}${su}`;
}

export function clock(ms: number): string {
  const total = Math.max(0, Math.round(ms / 1000));
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, '0')}`;
}

export const time = (ts: number) =>
  new Intl.DateTimeFormat(uiLocale(), { hour: '2-digit', minute: '2-digit' }).format(new Date(ts));

export const cacheDate = (ts: number) =>
  new Intl.DateTimeFormat(uiLocale(), {
    day: 'numeric',
    month: 'long',
    hour: '2-digit',
    minute: '2-digit',
  }).format(new Date(ts));

/** Дата и время коротко: «11 окт., 19:38» / «Oct 11, 7:38 PM» — для узких таблиц. */
export const shortDate = (ts: number) =>
  new Intl.DateTimeFormat(uiLocale(), {
    day: 'numeric',
    month: 'short',
    hour: 'numeric',
    minute: '2-digit',
  }).format(new Date(ts));

/** Короткая подпись дня: «12 окт» / «Oct 12». */
export const dayLabel = (ts: number) =>
  new Intl.DateTimeFormat(uiLocale(), { day: 'numeric', month: 'short' })
    .format(new Date(ts))
    .replace('.', '');

/** Полная подпись дня: «пн, 12 октября» / «Mon, October 12». */
export const dayFull = (ts: number) =>
  new Intl.DateTimeFormat(uiLocale(), { weekday: 'short', day: 'numeric', month: 'long' }).format(
    new Date(ts),
  );

/** Возраст события человеческим языком. */
export function ago(ms: number): string {
  if (ms < 8_000) return tr('только что', 'just now');
  if (ms < 60_000) return tr(`${Math.round(ms / 1000)} с назад`, `${Math.round(ms / 1000)}s ago`);
  if (ms < 3_600_000) {
    return tr(`${Math.round(ms / 60_000)} мин назад`, `${Math.round(ms / 60_000)} min ago`);
  }
  return tr(`${Math.round(ms / 3_600_000)} ч назад`, `${Math.round(ms / 3_600_000)}h ago`);
}

/**
 * Копия в буфер с запасным путём: Clipboard API может быть запрещён
 * политикой окружения, execCommand работает и там.
 */
export async function copyText(text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    const area = document.createElement('textarea');
    area.value = text;
    area.style.position = 'fixed';
    area.style.opacity = '0';
    document.body.appendChild(area);
    area.select();
    document.execCommand('copy');
    area.remove();
  }
}
