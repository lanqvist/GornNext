import { percentile, perHour, summarize, type RateSummary } from './stats.js';
import { FE_ID, type ConfigBaseId, type ItemDelta, type Run } from './types.js';

export type PriceLookup = (id: ConfigBaseId) => number | null;

/**
 * Цены для набора забегов: одна таблица на всех или своя на каждый забег.
 * Экономика каждый сезон начинается заново, и забег прошлого сезона,
 * оценённый нынешними ценами, показывал бы цифры, которых не было.
 */
export type RunPricing = PriceLookup | { forRun: (run: Run) => PriceLookup };

/** Таблица цен, по которой оценивается этот забег. */
export function pricesFor(pricing: RunPricing, run: Run): PriceLookup {
  return typeof pricing === 'function' ? pricing : pricing.forRun(run);
}

/**
 * Забег, который идёт в статистику фарма: не город и не мгновенный
 * проход через зону — пять секунд и меньше бывают только загрузкой.
 */
export function isFarmingRun(run: Run): boolean {
  return !run.isHub && (run.durationMs ?? 0) > 5000;
}

/**
 * Сезонная «Тюрьма Талоса». Лог видит у неё только финальную комнату
 * (36–100 с), а игра на доске проходит вне карт и во время не попадает:
 * 171 ФЕ за «1 м 40 с» давали 6 141 в час. Поэтому тюрьма — отдельный
 * источник дохода: в итогах ФЕ она есть, во всём «в час», медианах,
 * рекордах, картах и стратегиях — нет.
 */
export function isPrisonRun(run: Pick<Run, 'asset'>): boolean {
  return run.asset?.startsWith('ZhongZuiDiYu') ?? false;
}

/** Забег, который идёт в темп: фарм без тюрьмы. */
export function isRatedRun(run: Run): boolean {
  return isFarmingRun(run) && !isPrisonRun(run);
}

/** Сколько самой валюты ФЕ упало за забег — без оценки предметов. */
export function runFe(run: Run): number {
  let fe = 0;
  for (const d of run.drops) if (d.configBaseId === FE_ID) fe += d.qty;
  return fe;
}

/** Когда забег закончился — по этой границе он относится к сессии и дню. */
export function runEnd(run: Run): number {
  return run.endedAt ?? run.startedAt;
}

/** Ключ забега для дедупликации: один и тот же лог может разбираться дважды. */
export function runKey(run: Run): string {
  return `${run.startedAt}:${run.areaId}`;
}

/** Слияние списков забегов по ключу: при совпадении побеждает более поздний список. */
export function mergeRuns(...lists: Run[][]): Run[] {
  const byKey = new Map<string, Run>();
  for (const list of lists) for (const run of list) byKey.set(runKey(run), run);
  return dropNested([...byKey.values()].sort((a, b) => a.startedAt - b.startedAt));
}

/**
 * Убирает забеги, целиком лежащие внутри другого забега той же зоны.
 *
 * Один разбор таких не даёт: внутри карты между уходом в город и возвратом
 * бывают только хабы. Они остаются от прошлых версий: трекер, запущенный
 * между заходами через портал, записывал вторую половину карты отдельным
 * забегом, а следующий полный разбор склеивал карту целиком — и половина
 * с другим ключом считалась второй раз. На входе — забеги по началу.
 */
function dropNested(sorted: Run[]): Run[] {
  // Самый поздний конец закрытой карты зоны среди начатых раньше.
  const reach = new Map<number, number>();
  return sorted.filter((run) => {
    if (run.isHub || run.endedAt === null) return true;
    const end = reach.get(run.areaId);
    if (end !== undefined && run.endedAt <= end) return false;
    reach.set(run.areaId, run.endedAt);
    return true;
  });
}

/**
 * Сколько времени карта была «живой».
 *
 * Простой — главный способ незаметно испортить любую цифру «в час»:
 * отошёл на десять минут, не выходя из карты, и знаменатель вырос,
 * а числитель нет. Игра при этом продолжает писать в лог, поэтому
 * активность файла признаком не является — считаем по событиям добычи.
 *
 * Промежутки между событиями обрезаются порогом: пауза дольше него
 * в зачёт времени не идёт.
 */
export const IDLE_CAP_MS = 180_000;

/**
 * Активное время закрытых забегов. Срез спрашивает его по нескольку раз
 * на забег (темп, стратегии, карты, дни, сессии), а сортировка событий
 * у тысяч забегов — заметная доля всего среза. Забег, вернувшийся в карту
 * порталом, снова открывается и дописывается — поэтому запись сверяется
 * с концом и числом событий.
 */
const activeCache = new WeakMap<Run, { end: number; events: number; value: number }>();

export function activeDuration(run: Run, idleCapMs = IDLE_CAP_MS, until?: number): number {
  const cacheable = run.endedAt !== null && idleCapMs === IDLE_CAP_MS;
  const events = run.drops.length + run.costs.length;
  if (cacheable) {
    const hit = activeCache.get(run);
    if (hit && hit.end === run.endedAt && hit.events === events) return hit.value;
  }
  const value = computeActive(run, idleCapMs, until);
  if (cacheable) activeCache.set(run, { end: run.endedAt!, events, value });
  return value;
}

function computeActive(run: Run, idleCapMs: number, until?: number): number {
  const end = run.endedAt ?? until ?? run.startedAt;
  const total = Math.max(0, end - run.startedAt);
  if (total <= idleCapMs) return total;

  const events = [...run.drops, ...run.costs]
    .map((e) => e.ts)
    .filter((ts) => ts >= run.startedAt && ts <= end)
    .sort((a, b) => a - b);

  // Без событий судить не о чем — карта могла быть пустой, а могла быть
  // брошенной. Обрезаем по порогу, но не выбрасываем целиком.
  if (events.length === 0) return Math.min(total, idleCapMs);

  let active = 0;
  let previous = run.startedAt;
  for (const ts of [...events, end]) {
    active += Math.min(ts - previous, idleCapMs);
    previous = ts;
  }
  return active;
}

/** Чистая стоимость забега: добыча минус вложения, обе по референсным ценам. */
export function runNet(run: Run, pricing: RunPricing): number {
  const priceOf = pricesFor(pricing, run);
  let value = 0;
  for (const d of run.drops) {
    const price = priceOf(d.configBaseId);
    if (price !== null) value += price * d.qty;
  }
  let cost = 0;
  for (const c of run.costs) {
    const price = priceOf(c.configBaseId);
    if (price !== null) cost += price * c.qty;
  }
  return value - cost;
}

/**
 * Подпись стратегии: из чего карта была открыта.
 *
 * Вложения известны поштучно — они видны в логе как расход по протоколу
 * Spv3Open. Значит стратегию можно определить автоматически, а не просить
 * игрока размечать забеги руками. Exile Diary завёл теги стратегий
 * с постоянной ценой карты; здесь и состав, и цена берутся из фактических
 * расходов.
 */
export function strategyKey(run: Run): string {
  const ids = [...new Set(run.costs.map((c) => c.configBaseId))].sort((a, b) => a - b);
  return ids.length === 0 ? 'none' : ids.join('+');
}

export interface StrategyStats {
  key: string;
  /** Идентификаторы предметов, которыми открывалась карта. */
  costItems: ConfigBaseId[];
  runs: number;
  rate: RateSummary;
  /** Средние вложения на карту. */
  costPerRun: number;
  /** Средняя чистая прибыль с карты. */
  netPerRun: number;
  totalActiveMs: number;
}

/**
 * Группировка забегов по стратегии с честной статистикой внутри каждой.
 *
 * Стратегии с одним-двумя забегами показывать как сравнимые нельзя,
 * поэтому размер выборки едет вместе с цифрой, а не отдельно.
 */
export function groupByStrategy(
  runs: Run[],
  pricing: RunPricing,
  idleCapMs = IDLE_CAP_MS,
): StrategyStats[] {
  const groups = new Map<string, Run[]>();
  for (const run of runs) {
    const key = strategyKey(run);
    const list = groups.get(key);
    if (list) list.push(run);
    else groups.set(key, [run]);
  }

  const out: StrategyStats[] = [];
  for (const [key, list] of groups) {
    const rates: number[] = [];
    let cost = 0;
    let net = 0;
    let activeMs = 0;

    for (const run of list) {
      const priceOf = pricesFor(pricing, run);
      const active = activeDuration(run, idleCapMs);
      const runValue = runNet(run, priceOf);
      activeMs += active;
      net += runValue;
      for (const c of run.costs) {
        const price = priceOf(c.configBaseId);
        if (price !== null) cost += price * c.qty;
      }
      if (active > 5000) rates.push(perHour(runValue, active));
    }

    out.push({
      key,
      costItems: key === 'none' ? [] : key.split('+').map(Number),
      runs: list.length,
      rate: summarize(rates),
      costPerRun: cost / list.length,
      netPerRun: net / list.length,
      totalActiveMs: activeMs,
    });
  }

  // Сначала то, во что вложено больше всего времени: по нему и данных больше.
  return out.sort((a, b) => b.totalActiveMs - a.totalActiveMs);
}

export interface MissingPrice {
  configBaseId: ConfigBaseId;
  /** Сколько штук уже выпало или потрачено. */
  qty: number;
  /** В скольких забегах встречался. */
  runs: number;
  /** Тратится на открытие карт — такой предмет искажает прибыль напрямую. */
  isCost: boolean;
}

/**
 * Чего не хватает для точной оценки — по влиянию на цифру.
 *
 * Порядок не по количеству штук: 305 Пламенных песков по 0,0001 стоят
 * три сотых ФЕ, а неоценённый маяк тратится на каждую карту и искажает
 * прибыль напрямую. Поэтому сначала расходники, затем то, что встречается
 * в большем числе забегов, и только потом количество.
 */
export function missingPrices(
  runs: Run[],
  pricing: RunPricing,
  base?: MissingTally,
): MissingPrice[] {
  const totals = tallyMissing(runs, pricing, base ? copyTally(base) : new Map());
  return [...totals.entries()]
    .map(([configBaseId, e]) => ({ configBaseId, ...e }))
    .sort((a, b) => {
      if (a.isCost !== b.isCost) return a.isCost ? -1 : 1;
      if (a.runs !== b.runs) return b.runs - a.runs;
      return b.qty - a.qty;
    });
}

/**
 * Недостающие цены, ещё не упорядоченные: закрытые забеги считаются
 * один раз, а открытый досчитывается поверх копии на каждом срезе.
 * Порядок вставки сохраняется — при равенстве он решает и в missingPrices.
 */
export type MissingTally = Map<ConfigBaseId, Omit<MissingPrice, 'configBaseId'>>;

export function tallyMissing(
  runs: Run[],
  pricing: RunPricing,
  into: MissingTally = new Map(),
): MissingTally {
  for (const run of runs) {
    const priceOf = pricesFor(pricing, run);
    const inRun = new Set<ConfigBaseId>();
    const note = (delta: ItemDelta, isCost: boolean): void => {
      if (priceOf(delta.configBaseId) !== null) return;
      let entry = into.get(delta.configBaseId);
      if (!entry) into.set(delta.configBaseId, (entry = { qty: 0, runs: 0, isCost: false }));
      entry.qty += delta.qty;
      if (!inRun.has(delta.configBaseId)) {
        inRun.add(delta.configBaseId);
        entry.runs++;
      }
      entry.isCost ||= isCost;
    };
    for (const drop of run.drops) note(drop, false);
    for (const cost of run.costs) note(cost, true);
  }
  return into;
}

function copyTally(tally: MissingTally): MissingTally {
  return new Map([...tally].map(([id, e]) => [id, { ...e }]));
}

/**
 * Норма, с которой сравнивается забег, — медиана последних N забегов.
 *
 * Сравнивать с лучшим забегом статистически бессмысленно и вдобавок
 * демотивирует: рекорд по определению недостижим в большинстве попыток.
 * Скользящее окно нужно, чтобы вчерашняя кампания не тянула норму вниз,
 * когда сегодня фармится эндгейм.
 */
export const NORM_WINDOW = 20;

export function rollingNorm(rates: number[], window = NORM_WINDOW): number {
  const recent = rates.slice(-window);
  if (recent.length === 0) return 0;
  return percentile([...recent].sort((a, b) => a - b), 0.5);
}

/** Дорогая находка сессии: предмет в одном забеге, все его штуки вместе. */
export interface TopDrop {
  configBaseId: ConfigBaseId;
  qty: number;
  totalFe: number;
  /** Когда выпала последняя штука. */
  at: number;
  run: Run;
}

/**
 * Самые дорогие находки по забегам — для «крупного за сессию» в оверлее.
 *
 * Плашка джекпота живёт секунды, а потом дорогая находка пропадает из виду
 * до конца забега. Здесь она остаётся. Сам ФЕ не в счёт: сотня монет,
 * собранная по одной за карту, — это доход, а не находка.
 *
 * Сессия без сброса — вся история сезона, поэтому без сортировки всего
 * подряд: держим только `limit` лучших.
 */
export function topDrops(
  runs: Run[],
  pricing: RunPricing,
  limit = 3,
  /** Лучшие из уже просмотренных забегов: открытый досчитывается поверх. */
  seed: TopDrop[] = [],
): TopDrop[] {
  const top = [...seed];
  for (const run of runs) {
    const priceOf = pricesFor(pricing, run);
    const byItem = new Map<ConfigBaseId, { qty: number; at: number }>();
    for (const d of run.drops) {
      if (d.configBaseId === FE_ID) continue;
      const e = byItem.get(d.configBaseId) ?? { qty: 0, at: 0 };
      e.qty += d.qty;
      e.at = Math.max(e.at, d.ts);
      byItem.set(d.configBaseId, e);
    }
    for (const [configBaseId, e] of byItem) {
      const unit = priceOf(configBaseId);
      if (unit === null || unit <= 0 || e.qty <= 0) continue;
      const totalFe = unit * e.qty;
      if (top.length === limit && totalFe <= top[limit - 1]!.totalFe) continue;
      top.push({ configBaseId, qty: e.qty, totalFe, at: e.at, run });
      top.sort((a, b) => b.totalFe - a.totalFe);
      if (top.length > limit) top.pop();
    }
  }
  return top;
}
