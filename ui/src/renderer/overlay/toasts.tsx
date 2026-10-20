import { useEffect, useRef, useState } from 'react';

import { tr } from '../../core/i18n.js';
import type { DropView, RunView } from '../../main/engine.js';
import type { ToastSnapshot } from '../../main/toastFeed.js';
import { Fe, FeIcon } from '../components/Fe.js';
import { clock } from '../format.js';
import { playDrop, playRecord } from './sound.js';

/**
 * Всплывающий доход в оверлее.
 *
 * Событий «подобран предмет» в срезе нет — есть только состояния. Поэтому
 * всплывашки вычисляются диффом соседних срезов текущего забега:
 * вырос netFe — значит, что-то подобрано; пропал current — забег закрыт.
 *
 * Дизайн-решения:
 *  - подборы в пределах короткого окна СХЛОПЫВАЮТСЯ в одну всплывашку
 *    с растущей суммой: лут-пылесос после босса даёт десятки изменений
 *    в секунду, и очередь из тридцати плашек — это спам, а не кайф;
 *  - дорогая находка (порог ниже) получает золотую плашку с именем
 *    предмета и отдельный звук — именно этот момент игрок хочет заметить;
 *  - закрытие карты подводит итог одной строкой; поражение не озвучивается
 *    фанфарами.
 */

/**
 * Ступени подбора — как в лут-фильтрах PoE: мелочь (pickup) — короткая цифра,
 * оранжевое (notable) — с иконкой и названием, золото (jackpot) — карточка.
 */
export type ToastKind = 'pickup' | 'notable' | 'jackpot' | 'runEnd' | 'prison';

/** Ступень ценности — те же пороги и цвета, что у полос дропа в оверлее. */
export type Tier = 'gold' | 'orange' | 'plain';

export interface Toast {
  id: number;
  kind: ToastKind;
  fe: number;
  /** Название предмета — у оранжевых и золотых плашек. */
  name?: string;
  icon?: string | null;
  /** Длительность забега — только у итога карты. */
  durationMs?: number;
  /** Топ находок закрытой карты — только у итога. */
  topDrops?: Array<{ name: string; icon: string | null; fe: number; tier: Tier }>;
  /** Отклонение темпа закрытой карты от нормы (медианы последних 20 забегов), в процентах. */
  deltaPct?: number | null;
  /** Стрик: не первый джекпот за короткое время — сессия «жаркая». */
  hot?: boolean;
  /** Лучший темп сессии — карта-рекорд, с монетным дождём. */
  record?: boolean;
  /** Цена самой дорогой находки в склеенной плашке — по ней выбирается название. */
  topFe?: number;
  /** Сколько ещё заметных находок склеено в оранжевую плашку. */
  more?: number;
  /** Плашка уходит: класс запускает затухание, через LEAVE_MS её не станет. */
  leaving?: boolean;
}

/** Порог «золотой» находки по умолчанию, в ФЕ. Настраивается игроком. */
export const JACKPOT_FE = 50;
/** Окно схлопывания подборов подряд. */
const MERGE_WINDOW_MS = 900;
/** Два джекпота внутри этого окна — «жаркий» стрик с искрами. */
const HOT_STREAK_MS = 90_000;
/** Монетный дождь на рекорд — не чаще раза в час, иначе приестся. */
const RECORD_COOLDOWN_MS = 3_600_000;
/** Сколько всплывашка живёт на экране. Итог карты читают — ему дольше. */
const TTL_MS: Record<ToastKind, number> = {
  pickup: 2400,
  notable: 3200,
  jackpot: 4200,
  runEnd: 5200,
  prison: 4200,
};
/**
 * Затухание перед исчезновением. Запускается классом по таймеру, а не
 * задержкой в CSS: склеенная плашка продлевает жизнь, и CSS-задержка
 * от первого подбора гасила её, пока лут ещё сыпался.
 */
const LEAVE_MS = 420;
/** Больше трёх плашек — уже стена текста поверх игры. */
const MAX_VISIBLE = 3;

export interface ToastOptions {
  /** Показывать плашки. Звук от них независим: можно слушать без визуала. */
  popups: boolean;
  sound: boolean;
  volume: number;
  /** Порог «золотой» находки, в ФЕ. */
  jackpotFe: number;
  /** Звук «возвышение» на предмет дороже этого порога, в ФЕ. 0 — выключен. */
  exaltedFe: number;
  /** Звук «дивайн» на предмет дороже этого порога, в ФЕ. 0 — выключен. */
  divineFe: number;
}

interface RunFingerprint {
  /** Последний вид забега: у тюрьмы итога в recent нет — берём отсюда. */
  view: RunView;
  startedAt: number;
  netFe: number;
  qtyByItem: Map<number, number>;
}

/** Три самых дорогих находки забега со ступенью ценности — для карточки итога. */
export function topDropsOf(
  drops: DropView[],
  goldFe: number,
  orangeFe: number,
): Array<{ name: string; icon: string | null; fe: number; tier: Tier }> {
  return drops
    .filter((d) => d.priceSource !== 'excluded' && d.totalFe !== null && d.totalFe > 0)
    .sort((a, b) => b.totalFe! - a.totalFe!)
    .slice(0, 3)
    .map((d) => ({
      name: d.name,
      icon: d.icon,
      fe: d.totalFe!,
      tier: d.totalFe! >= goldFe ? 'gold' : d.totalFe! >= orangeFe ? 'orange' : 'plain',
    }));
}

/**
 * Склейка подбора в живую плашку: сумма растёт, затухание отменяется,
 * ступень только повышается (мелочь — в оранжевую), название — у самой
 * дорогой находки пачки, остальные заметные считаются «и ещё N».
 */
export function mergePickup(
  was: Toast,
  gained: number,
  notable: { name: string; icon: string | null; fe: number } | null,
): Toast {
  const next: Toast = { ...was, fe: was.fe + gained, leaving: false };
  if (!notable) return next;
  next.kind = 'notable';
  next.more = was.kind === 'notable' ? (was.more ?? 0) + 1 : 0;
  if (was.kind === 'pickup' || notable.fe > (was.topFe ?? 0)) {
    next.name = notable.name;
    next.icon = notable.icon;
    next.topFe = notable.fe;
  }
  return next;
}

let nextId = 1;

export function useIncomeToasts(snapshot: ToastSnapshot | null, opts: ToastOptions): Toast[] {
  const [toasts, setToasts] = useState<Toast[]>([]);
  // Список дублируется в ref: ветке схлопывания нужно синхронно знать,
  // жива ли ещё плашка подбора, а state к этому моменту может отставать.
  const listRef = useRef<Toast[]>([]);
  // Таймеры гашения по id плашки — чтобы при схлопывании продлевать жизнь,
  // а не давать плашке умереть по таймеру от самого первого подбора.
  // Два таймера на плашку: начать затухание и убрать.
  const timers = useRef(new Map<number, Array<ReturnType<typeof setTimeout>>>());
  const prev = useRef<RunFingerprint | null>(null);
  const lastPickupAt = useRef(0);
  const lastJackpotAt = useRef(0);
  const lastRecordAt = useRef(0);
  const optsRef = useRef(opts);
  optsRef.current = opts;

  useEffect(() => {
    const pending = timers.current;
    return () => {
      for (const pair of pending.values()) pair.forEach(clearTimeout);
      pending.clear();
    };
  }, []);

  useEffect(() => {
    const current = snapshot?.current ?? null;
    const before = prev.current;
    prev.current = current
      ? {
          view: current,
          startedAt: current.startedAt,
          netFe: current.netFe,
          qtyByItem: new Map(current.drops.map((d) => [d.configBaseId, d.qty])),
        }
      : null;

    const { popups, sound, volume, jackpotFe, exaltedFe, divineFe } = optsRef.current;
    if (!popups && !sound) return;

    const commit = (list: Toast[]): void => {
      listRef.current = list;
      setToasts(list);
    };

    // (Пере)взводит таймер гашения: старый таймер той же плашки снимается.
    const scheduleExpiry = (id: number, kind: ToastKind): void => {
      timers.current.get(id)?.forEach(clearTimeout);
      const ttl = TTL_MS[kind];
      timers.current.set(id, [
        setTimeout(() => {
          commit(listRef.current.map((t) => (t.id === id ? { ...t, leaving: true } : t)));
        }, ttl - LEAVE_MS),
        setTimeout(() => {
          timers.current.delete(id);
          commit(listRef.current.filter((t) => t.id !== id));
        }, ttl),
      ]);
    };

    const push = (toast: Toast): void => {
      if (!popups) return;
      commit([toast, ...listRef.current].slice(0, MAX_VISIBLE));
      scheduleExpiry(toast.id, toast.kind);
    };

    // Забег закрыт: current исчез или сменился новым стартом.
    // Момент закрытия карты — естественная пауза, игрок готов читать,
    // поэтому итог — карточка с топ-находками, а не одна строка.
    if (before && (!current || current.startedAt !== before.startedAt)) {
      const goldFe = jackpotFe > 0 ? jackpotFe : (snapshot?.jackpotAutoFe ?? JACKPOT_FE);
      const orangeFe = exaltedFe > 0 ? exaltedFe : goldFe / 5;
      const top = (drops: DropView[]) => topDropsOf(drops, goldFe, orangeFe);

      // Тюрьма в «последние забеги» не входит — её итог из последнего вида
      // забега, без темпа и процентов: время на доске лог не видит.
      if (before.view.prison) {
        push({
          id: nextId++,
          kind: 'prison',
          fe: before.view.netFe,
          topDrops: top(before.view.drops),
        });
      }

      const finished = before.view.prison
        ? undefined
        : snapshot?.recent.find((r) => r.startedAt === before.startedAt);
      if (finished) {
        // К норме — медиане последних 20 забегов, как в главном окне
        // и таблице забегов. Медиана всего сезона копит слабые первые
        // недели, и почти каждая карта выходила бы «+150% к норме».
        const norm = snapshot?.norm ?? 0;

        // Рекорд сессии: темп выше всех остальных недавних забегов.
        // Кулдаун держит его событием, а не ежечасным фоном.
        const others = (snapshot?.recent ?? []).filter(
          (r) => r.startedAt !== finished.startedAt,
        );
        const bestOther = others.reduce((a, r) => Math.max(a, r.perHour), 0);
        const record =
          others.length >= 4 &&
          finished.perHour > bestOther &&
          Date.now() - lastRecordAt.current > RECORD_COOLDOWN_MS;
        if (record) lastRecordAt.current = Date.now();

        push({
          id: nextId++,
          kind: 'runEnd',
          fe: finished.netFe,
          durationMs: finished.durationMs ?? 0,
          record,
          topDrops: top(finished.drops),
          deltaPct: norm > 0 ? Math.round(((finished.perHour - norm) / norm) * 100) : null,
        });
        // Звучит только рекорд: обычный итог карты молчит, его читают глазами.
        if (sound && record) playRecord(volume);
      }
    }

    if (!current || !before || current.startedAt !== before.startedAt) return;

    const gained = current.netFe - before.netFe;
    // Отрицательная дельта — пересчёт цен или расход, не подбор.
    if (gained <= 0.005) return;

    // Порог «золота»: явный из настроек либо «авто» — P95 цен сезона,
    // чтобы золотая плашка оставалась событием и в зрелой экономике.
    // «Оранжевое» — те же пороги, что у полос дропа в оверлее.
    const goldFe = jackpotFe > 0 ? jackpotFe : (snapshot?.jackpotAutoFe ?? JACKPOT_FE);
    const orangeFe = exaltedFe > 0 ? exaltedFe : goldFe / 5;

    // Ступень — по конкретному предмету, а не по сумме дельты: пачка
    // мелочи на 60 ФЕ — обычный подбор, а не джекпот.
    let top: { name: string; icon: string | null; fe: number } | null = null;
    for (const d of current.drops) {
      const gainedQty = d.qty - (before.qtyByItem.get(d.configBaseId) ?? 0);
      if (gainedQty <= 0 || d.unitFe === null) continue;
      const value = gainedQty * d.unitFe;
      if (top === null || value > top.fe) {
        const name = gainedQty > 1 ? `${d.name} ×${gainedQty}` : d.name;
        top = { name, icon: d.icon, fe: value };
      }
    }
    const priciest = top?.fe ?? 0;

    if (top && priciest >= goldFe) {
      // Стрик: второй джекпот подряд получает искры и звон закалки.
      const hot = Date.now() - lastJackpotAt.current < HOT_STREAK_MS;
      lastJackpotAt.current = Date.now();
      push({
        id: nextId++,
        kind: 'jackpot',
        fe: top.fe,
        name: top.name,
        icon: top.icon,
        hot,
      });
      // Золотая находка всегда дороже порогов звука — звучит «дивайн».
      if (sound) playDrop('divine', volume);
      return;
    }

    const now = Date.now();
    const merged = now - lastPickupAt.current < MERGE_WINDOW_MS;
    lastPickupAt.current = now;

    // Дорогой, но не золотой предмет — как в фильтрах PoE: что упало, слышно
    // не глядя. Дороже второго порога — «дивайн», первого — «возвышение».
    // Остальные подборы молчат: звук на каждый тик — фон, а не событие.
    if (sound && divineFe > 0 && priciest >= divineFe) playDrop('divine', volume);
    else if (sound && exaltedFe > 0 && priciest >= exaltedFe) playDrop('exalted', volume);
    if (!popups) return;

    const notable = top && priciest >= orangeFe ? top : null;

    // Лут-пылесос после босса — десятки подборов в секунду. Подряд идущие
    // склеиваются в одну плашку: сумма растёт, ступень только повышается
    // (мелочь — в оранжевую), название — у самой дорогой находки пачки.
    const idx = merged
      ? listRef.current.findIndex((t) => t.kind === 'pickup' || t.kind === 'notable')
      : -1;
    if (idx !== -1) {
      const next = mergePickup(listRef.current[idx]!, gained, notable);
      const updated = [...listRef.current];
      updated[idx] = next;
      commit(updated);
      scheduleExpiry(next.id, next.kind);
      return;
    }

    push(
      notable
        ? {
            id: nextId++,
            kind: 'notable',
            fe: gained,
            name: notable.name,
            icon: notable.icon,
            topFe: notable.fe,
            more: 0,
          }
        : { id: nextId++, kind: 'pickup', fe: gained },
    );
  }, [snapshot]);

  return toasts;
}

export function ToastList({ toasts }: { toasts: Toast[] }) {
  if (toasts.length === 0) return null;
  return (
    <div className="ov-toasts">
      {toasts.map((t) => (
        <div
          key={t.id}
          className={`ov-toast ov-toast-${t.kind} ${t.leaving ? 'ov-toast-leaving' : ''} ${
            t.hot ? 'ov-toast-hot' : ''
          } ${
            t.record ? 'ov-toast-record' : ''
          }`}
        >
          {/* Искры стрика: чистый CSS, по спану на искру. */}
          {t.hot && (
            <span className="ov-sparks" aria-hidden="true">
              {Array.from({ length: 6 }, (_, i) => (
                <i key={i} />
              ))}
            </span>
          )}
          {/* Монетный дождь рекорда: те же иконки ФЕ, что и в числах. */}
          {t.record && (
            <span className="ov-coins" aria-hidden="true">
              {Array.from({ length: 7 }, (_, i) => (
                <i key={i}>
                  <FeIcon />
                </i>
              ))}
            </span>
          )}
          {t.kind === 'jackpot' && (
            <>
              {t.icon && <img className="ov-toast-icon" src={t.icon} alt="" draggable={false} />}
              <span className="ov-toast-name">{t.name}</span>
            </>
          )}
          {t.kind === 'notable' && (
            <>
              {t.icon && <img className="ov-toast-icon" src={t.icon} alt="" draggable={false} />}
              <span className="ov-toast-name">
                {t.name}
                {(t.more ?? 0) > 0 && (
                  <span className="ov-toast-more">
                    {' '}
                    {tr(`и ещё ${t.more}`, `+${t.more} more`)}
                  </span>
                )}
              </span>
            </>
          )}
          {(t.kind === 'pickup' || t.kind === 'notable' || t.kind === 'jackpot') && (
            <Fe value={t.fe} signed className="ov-toast-fe" />
          )}
          {t.kind === 'prison' && (
            <div className="ov-runend">
              <div className="ov-runend-head">
                <span className="ov-toast-label">{tr('тюрьма', 'prison')}</span>
                <Fe value={t.fe} signed className="ov-toast-fe" />
                <span className="ov-toast-time">
                  {tr('в итог сессии, не в темп', 'counts toward the session, not the pace')}
                </span>
              </div>
              {t.topDrops && t.topDrops.length > 0 && <TopDrops drops={t.topDrops} />}
            </div>
          )}
          {t.kind === 'runEnd' && (
            <div className="ov-runend">
              <div className="ov-runend-head">
                <span className={`ov-toast-label ${t.record ? 'ov-record-label' : ''}`}>
                  {t.record
                    ? tr('рекорд сессии!', 'session record!')
                    : tr('карта закрыта', 'map closed')}
                </span>
                <Fe value={t.fe} signed className="ov-toast-fe" />
                {t.durationMs !== undefined && (
                  <span className="ov-toast-time">{clock(t.durationMs)}</span>
                )}
                {t.deltaPct != null && t.deltaPct !== 0 && (
                  <span className={`ov-runend-delta ${t.deltaPct > 0 ? 'up' : 'down'}`}>
                    {t.deltaPct > 0 ? '+' : '−'}
                    {Math.abs(t.deltaPct)}% {tr('к норме', 'vs norm')}
                  </span>
                )}
              </div>
              {t.topDrops && t.topDrops.length > 0 && <TopDrops drops={t.topDrops} />}
              {/* Выход с карты — пауза: самое время заглянуть в пульт. */}
              <span className="ov-runend-hint">
                {tr('подробнее —', 'details —')} <kbd>Ctrl+Alt+T</kbd>
              </span>
            </div>
          )}
        </div>
      ))}
    </div>
  );
}

/** Находки итога: иконка, название и цена, кромка — ступень ценности, как в оверлее. */
function TopDrops({ drops }: { drops: NonNullable<Toast['topDrops']> }) {
  return (
    <div className="ov-runend-drops">
      {drops.map((d, i) => (
        <span key={i} className={`ov-runend-drop t-${d.tier}`}>
          {d.icon && <img src={d.icon} alt="" draggable={false} />}
          <span className="ov-runend-name">{d.name}</span>
          <Fe value={d.fe} />
        </span>
      ))}
    </div>
  );
}
