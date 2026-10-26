/**
 * Типы среза движка для интерфейса — договор с Rust (crates/gorn-core/src/engine.rs,
 * сериализация serde camelCase). Перенесены из main/engine.ts эталона без логики.
 */
import type { RateSummary } from '../core/stats.js';
import type { StrategyStats } from '../core/analysis.js';
import type { ConfigBaseId } from '../core/types.js';

export interface MapStats {
  /** Имя карты — то же, что в таблице забегов. */
  name: string;
  runs: number;
  rate: RateSummary;
  netPerRun: number;
  /** Среднее активное время карты. */
  avgActiveMs: number;
  totalActiveMs: number;
}
export interface PeriodTotals {
  /** Забеги в темп — без тюрьмы. */
  runs: number;
  /** Итог, вместе с тюрьмой. */
  netFe: number;
  /** Время в картах — без тюрьмы: её время лог не видит. */
  inMapMs: number;
  /** Тюрьма отдельно: в netFe она есть, в темп (netFe − prisonFe) / inMapMs — нет. */
  prisonFe: number;
  prisonRuns: number;
}
export interface DayTotals extends PeriodTotals {
  /** Полночь местного времени. */
  day: number;
}
export interface SessionBounds {
  startedAt: number;
  /** null — активная сессия. */
  endedAt: number | null;
}
export interface SessionTotals extends PeriodTotals, SessionBounds {}

export type PriceSource = 'own' | 'community';

export interface PricePoint {
  t: number;
  fe: number;
}
export interface PriceTrend {
  /** Точки за последние 30 дней от свежей — для спарклайна, старые слева. */
  points: PricePoint[];
  /**
   * Изменение свежей цены к самой ранней пробивке в пределах недели
   * до неё, долей: 0,12 — подорожал на 12%. null — сравнивать не с чем.
   */
  change: number | null;
}
export interface DropView {
  configBaseId: ConfigBaseId;
  name: string;
  icon: string | null;
  qty: number;
  unitFe: number | null;
  totalFe: number | null;
  /** Откуда цена: своя пробивка, база сообщества или предмет исключён из учёта. */
  priceSource: ViewPriceSource | null;
}

/** Источник цены для интерфейса: исключённый предмет стоит ноль по воле игрока. */
export type ViewPriceSource = PriceSource | 'excluded';

type PickedPrice = { fe: number; source: ViewPriceSource };

/** Строка блока «Цены». */
export interface PriceRow {
  configBaseId: ConfigBaseId;
  name: string;
  icon: string | null;
  /** Цена, по которой сейчас считается добыча. null — неизвестна. */
  fe: number | null;
  source: ViewPriceSource | null;
  /** Когда пробил сам — по свежей пробивке. null — своих пробивок нет. */
  ownObservedAt: number | null;
  /** Тренд по своим пробивкам. */
  trend: PriceTrend;
  /** Сколько выпало и ушло на карты в видимых забегах. */
  seenQty: number;
}

/** Стоимость сумки для интерфейса. */
/** Слот сумки с ценой: за штуку (unitFe) и за стопку (totalFe). */
export interface BagSlotView extends DropView {
  slotId: number;
  /** Предмет есть в справочнике. Вне его — то, чем нельзя торговать: ни названия, ни иконки, ни цены. */
  known: boolean;
}

/** Страница сумки: 100 снаряжение, 101 навыки, 102 материалы, 103 прочее; ниже 100 — тайник. */
export interface BagPageView {
  pageId: number;
  valueFe: number;
  /** Занятых слотов без цены (непродаваемое вне справочника не считается). */
  unpriced: number;
  slots: BagSlotView[];
}

/** Сумка по слотам — по запросу `bag:query`, не в каждом срезе. */
export interface BagGrid {
  pages: BagPageView[];
}

export interface BagView {
  valueFe: number;
  feCash: number;
  unpriced: number;
  top: Array<{
    configBaseId: ConfigBaseId;
    name: string;
    icon: string | null;
    qty: number;
    totalFe: number;
  }>;
}

export interface RunView {
  /** Ключ забега: стабилен между срезами, годится для React key. */
  key: string;
  areaId: number;
  name: string;
  startedAt: number;
  /** Полная длительность — для таймера. У открытого забега null. */
  durationMs: number | null;
  /**
   * Активное время без простоя (activeDuration), у открытого — до сейчас.
   * По нему считаются perHour, норма и поиск: иначе забег
   * с перекуром сравнивался с нормой, из которой перекуры вычтены.
   */
  activeMs: number;
  feGained: number;
  valueFe: number;
  costFe: number;
  netFe: number;
  perHour: number;
  drops: DropView[];
  /** Чем открывалась карта. */
  costs: DropView[];
  /** Тюрьма Талоса: в итоге сессии есть, в темпе нет (isPrisonRun). */
  prison?: boolean;
}

/** Результат поиска по всей истории забегов. */
export interface RunQueryResult {
  /** Сколько забегов подошло всего — показанных бывает меньше. */
  total: number;
  netFe: number;
  inMapMs: number;
  rate: RateSummary;
  /** Свежие сверху, не больше RUN_QUERY_LIMIT. */
  runs: RunView[];
}

/** Сколько забегов поиск отдаёт в интерфейс: итоги считаются по всем. */
export const RUN_QUERY_LIMIT = 100;

/** Персонаж для показа в интерфейсе. */
export interface PlayerView {
  playerId: string;
  /** Ник аккаунта без решёточного суффикса: «VLPZDC», не «VLPZDC#3648». */
  name: string | null;
  /** Имя класса или «Герой N», если id ещё не в словаре. */
  heroName: string | null;
  level: number | null;
  /** Сезон персонажа: в новом сезоне — новый персонаж. */
  seasonId: number | null;
  seasonName: string | null;
}

/** Персонаж с числом его забегов. */
export interface PlayerOption extends PlayerView {
  /** Фарм-забегов у персонажа — чтобы фильтр показывал, где есть данные. */
  runs: number;
}

export interface Snapshot {
  status: 'searching' | 'idle' | 'running';
  /** Пояснение статуса для игрока: почему ничего не происходит. */
  hint: string | null;
  /**
   * Когда лог последний раз дал распознанное событие; 0 — ещё ни разу.
   * Интерфейс считает «N с назад» сам — снапшот не обязан тикать.
   */
  lastActivityAt: number;
  /** Папка логов, с которой работает движок. null — игра не найдена. */
  logDir: string | null;
  /** Текущий сезон — по последнему профилю персонажа в логе. */
  seasonId: number | null;
  /** «SS14 Tartarus» — для шапки; null, пока сезон неизвестен. */
  seasonName: string | null;
  /**
   * Показывать забеги всех сезонов. false — только текущего: экономика
   * каждый сезон начинается заново, и темп прошлого сезона описывает
   * другую игру.
   */
  allSeasons: boolean;
  /** Фарм-забегов других сезонов — без них переключатель не нужен. */
  otherSeasonRuns: number;
  /** Кем зашли в игру. null — вход ещё не видели (или игра перезапущена). */
  player: PlayerView | null;
  /** Все известные персонажи. */
  players: PlayerOption[];
  /** Фильтр по персонажу — в интерфейсе его больше нет, всегда null. */
  playerFilter: string | null;
  current: RunView | null;
  recent: RunView[];
  /** Чистая добыча в час по забегам вне сессии тоже (perHour — главное). */
  rate: RateSummary;
  feRate: RateSummary;
  totals: {
    runs: number;
    inMapMs: number;
    feGained: number;
    valueFe: number;
    costFe: number;
    realizedFe: number;
    realizationRatio: number | null;
    /** Бумажная оценка того, что выставлено на продажу. Сравнима с realizedFe. */
    listedPaperFe: number;
  };
  /**
   * Темп за сессию, включая текущий забег.
   * Двигается сразу при подборе — иначе экран
   * выглядит замершим ровно в тот момент, когда что-то происходит.
   */
  liveRate: number;
  session: {
    /** Начало отсчёта счётчиков. 0 — сессию ещё ни разу не сбрасывали. */
    startedAt: number;
    /** Итог прошлой сессии — чтобы сброс не выглядел потерей цифр. */
    previous: { netFe: number; inMapMs: number; runs: number } | null;
  };
  /** Норма для сравнений — в час за последние 20 забегов вместе, а не рекорд. */
  norm: number;
  /**
   * Порог «дорогой находки» для режима «авто»: P95 всех известных цен
   * сезона. В начале сезона 50 ФЕ — событие, в конце — фон; этот порог
   * дрейфует вместе с экономикой.
   */
  jackpotAutoFe: number;
  strategies: StrategyView[];
  /** Доходность по картам — «куда идти», в отличие от стратегий «чем открывать». */
  maps: MapStats[];
  /** Итоги по дням за последние две недели, старые слева. */
  days: DayTotals[];
  /** Сессии с итогами, свежие сверху. */
  sessions: SessionTotals[];
  /** Стоимость сумки по последнему известному состоянию. null — сумку ещё не видели. */
  bag: BagView | null;
  /** Порог «мелочи» в списках добычи, ФЕ. 0 — показывать всё. */
  minDropFe: number;
  /** Предметы без цены, по влиянию на оценку. */
  missing: MissingPriceView[];
  /** Самые дорогие находки сессии, дорогие первыми, до трёх. */
  sessionTop: SessionDropView[];
  /**
   * Тюрьма за сессию: её добыча входит в totals.valueFe, но не в темп,
   * время в картах и число забегов — показывается отдельной строкой.
   */
  prison: { netFe: number; runs: number };
  diagnostics: {
    unknownProtos: Array<[string, number]>;
    pricedItems: number;
    unpricedDrops: number;
    /** Сколько предметов оценено по базе сообщества (titrack.ninja). */
    communityPrices: number;
    communityEnabled: boolean;
    /** Когда цены сообщества успешно сверялись с сетью; null — ни разу. */
    communityFetchedAt: number | null;
    /** Последняя сверка не удалась — данные из кэша. */
    communityStale: boolean;
  };
}

export interface StrategyView extends Omit<StrategyStats, 'costItems'> {
  /** Человекочитаемое имя: из чего открывалась карта. */
  title: string;
  costItems: Array<{ configBaseId: ConfigBaseId; name: string; icon: string | null }>;
}

/** Дорогая находка сессии — «крупное за сессию» в оверлее. */
export interface SessionDropView {
  configBaseId: ConfigBaseId;
  name: string;
  icon: string | null;
  qty: number;
  totalFe: number;
  /** Когда выпала последняя штука. */
  at: number;
  /** Карта, где выпала. */
  runName: string;
}

export interface MissingPriceView {
  configBaseId: ConfigBaseId;
  name: string;
  icon: string | null;
  qty: number;
  runs: number;
  /** Тратится на открытие карт — прибыль занижена ровно на его стоимость. */
  isCost: boolean;
}

interface ItemInfo {
  name: string;
  nameRu: string | null;
  icon: string | null;
}

/** Какие названия предметов показывать. */
export type NameLanguage = 'ru' | 'en' | 'both';
