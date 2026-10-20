import type { RunView, Snapshot } from './engine.js';

/**
 * Срез для слоя всплывашек — только то, что читают плашки и звук:
 * открытый забег, недавние (итог карты, рекорд), норма и авто-порог
 * «золота». Полный срез — ~60 КБ на пульс в окно, которому из него
 * нужна малая часть.
 */
export type ToastSnapshot = Pick<Snapshot, 'current' | 'recent' | 'norm' | 'jackpotAutoFe'>;

export function toastSnapshot(s: Snapshot): ToastSnapshot {
  return { current: s.current, recent: s.recent, norm: s.norm, jackpotAutoFe: s.jackpotAutoFe };
}

const dropsKey = (run: RunView) =>
  run.drops.map((d) => [d.configBaseId, d.qty, d.unitFe, d.totalFe, d.priceSource, d.name]);

/**
 * Отпечаток полей, от которых зависят плашки. Темп открытого забега
 * (perHour, длительность) меняется каждый пульс от часов и плашкам
 * не нужен — по нему срез «изменившимся» не считается.
 */
export function toastKey(s: ToastSnapshot): string {
  const cur = s.current;
  return JSON.stringify([
    s.norm,
    s.jackpotAutoFe,
    cur && [cur.startedAt, cur.netFe, cur.prison, dropsKey(cur)],
    s.recent.map((r) => [r.startedAt, r.netFe, r.perHour, r.durationMs, dropsKey(r)]),
  ]);
}

/** Отдаёт урезанный срез, если для плашек что-то изменилось, иначе null. */
export class ToastFeed {
  private lastKey: string | null = null;

  next(s: Snapshot): ToastSnapshot | null {
    const slim = toastSnapshot(s);
    const key = toastKey(slim);
    if (key === this.lastKey) return null;
    this.lastKey = key;
    return slim;
  }

  /** Новое окно ничего не видело — следующий срез уходит в любом случае. */
  reset(): void {
    this.lastKey = null;
  }
}
