/** Идентификатор предмета в игре. Совпадает с ConfigBaseId в логе и с id в Rt-data.json. */
export type ConfigBaseId = number;

/** Flame Elementium — основная торговая валюта. Проверено на логах: PageId 102, SlotId 0. */
export const FE_ID: ConfigBaseId = 100300;

/**
 * Связанные (непередаваемые) предметы всегда стоят 0 ФЕ: их нельзя продать.
 * Признак связанности — название в справочнике («Bound …», «绑定…»),
 * а не арифметика id: пары «id + 1000» в большинстве случаев оказываются
 * просто соседними предметами.
 */

/** Смысл транзакции. Определяется по ItemChange@ ProtoName. */
export type ProtoKind =
  | 'loot' // доход: подобрано с земли
  | 'mapCost' // расход: открытие карты
  | 'trade' // операции с биржей
  | 'neutral' // перекладывание, надевание, оценка — не доход и не расход
  | 'unknown'; // незнакомый протокол, попадает в самодиагностику

export interface LogEventBase {
  /** Момент события из метки лога, в миллисекундах epoch. */
  ts: number;
  /** Номер строки в файле — для диагностики и воспроизведения. */
  line: number;
}

/** BagMgr@:Modfy — слот сумки получил новое СОВОКУПНОЕ количество (не прирост). */
export interface BagModifyEvent extends LogEventBase {
  kind: 'bagModify';
  pageId: number;
  slotId: number;
  configBaseId: ConfigBaseId;
  /** Новый размер стака в слоте целиком. */
  num: number;
}

/** BagMgr@:InitBagData — снимок слота при полной синхронизации страницы. */
export interface BagInitEvent extends LogEventBase {
  kind: 'bagInit';
  pageId: number;
  slotId: number;
  configBaseId: ConfigBaseId;
  num: number;
}

/** BagMgr@:RemoveBagItem — слот очищен. */
export interface BagRemoveEvent extends LogEventBase {
  kind: 'bagRemove';
  pageId: number;
  slotId: number;
}

/**
 * ItemChange@ Add — новый экземпляр предмета лёг в слот. Единственный
 * сигнал о дропе в слот, пустовавший с самого входа: такие слоты
 * в InitBagData не попадают.
 */
export interface BagAddEvent extends LogEventBase {
  kind: 'bagAdd';
  pageId: number;
  slotId: number;
  configBaseId: ConfigBaseId;
  num: number;
}

/** ItemChange@ ProtoName=X start|end — рамка транзакции вокруг изменений сумки. */
export interface ItemChangeEvent extends LogEventBase {
  kind: 'itemChange';
  proto: string;
  phase: 'start' | 'end';
}

/** LevelMgr:_EnterLevelSuccess(N) — вход в зону. */
export interface EnterLevelEvent extends LogEventBase {
  kind: 'enterLevel';
  areaId: number;
}

/** LevelMgr@:LevelPath — путь ассета карты, даёт человекочитаемое имя. */
export interface LevelPathEvent extends LogEventBase {
  kind: 'levelPath';
  /** Например SD_YuanGuTongDao201 */
  asset: string;
}

/** LevelMgr@ LevelUid, LevelType, LevelId — уточнение типа зоны. */
export interface LevelIdsEvent extends LogEventBase {
  kind: 'levelIds';
  levelUid: number;
  levelType: number;
  levelId: number;
}

/** Ответ биржи на пробивку цены: наблюдённые цены активных лотов. */
export interface PriceObservationEvent extends LogEventBase {
  kind: 'priceObservation';
  /** Предмет, который пробивали (из +filters+1+refer запроса). */
  configBaseId: ConfigBaseId;
  /** Валюта лотов, обычно FE_ID. */
  currency: ConfigBaseId;
  /** Цены за единицу по активным лотам. */
  unitPrices: number[];
  synId: number;
}

/**
 * Вход персонажем и его профиль — для атрибуции забегов.
 *
 * Источника два: строка PlayerIdTrace при логине (только playerId, зато
 * мгновенно) и сокет-дамп GetPlayerData (полный профиль чуть позже).
 */
export interface PlayerInfoEvent extends LogEventBase {
  kind: 'playerInfo';
  /**
   * Идентификатор персонажа — СТРОКА. Значения вида 956082728577617920
   * больше Number.MAX_SAFE_INTEGER: Number растерял бы младшие разряды,
   * и два разных персонажа могли бы склеиться в одного.
   */
  playerId: string;
  /** Класс персонажа (HeroId из профиля), например 1700. */
  heroId?: number;
  level?: number;
  /** Ник аккаунта («VLPZDC#3648») — один на все персонажи. */
  name?: string;
  seasonId?: number;
}

/** Сводный профиль персонажа, накопленный из событий playerInfo. */
export interface PlayerProfile {
  playerId: string;
  /** Ник аккаунта — общий для всех персонажей. */
  name: string | null;
  heroId: number | null;
  level: number | null;
  seasonId: number | null;
  /** Последний раз видели в логе — по часам лога. */
  lastSeenAt: number;
}

/**
 * Дополняет профиль более свежими данными: известные значения затирают
 * старые, отсутствующие — нет. Вход персонажем несёт только id, и без
 * этого правила он стирал бы имя и уровень из профиля.
 */
export function mergePlayerProfile(
  base: PlayerProfile | undefined,
  next: PlayerProfile,
): PlayerProfile {
  return {
    playerId: next.playerId,
    name: next.name ?? base?.name ?? null,
    heroId: next.heroId ?? base?.heroId ?? null,
    level: next.level ?? base?.level ?? null,
    seasonId: next.seasonId ?? base?.seasonId ?? null,
    lastSeenAt: Math.max(next.lastSeenAt, base?.lastSeenAt ?? 0),
  };
}

/** Дополняет профиль событием playerInfo. */
export function mergePlayerInfo(
  base: PlayerProfile | undefined,
  ev: PlayerInfoEvent,
): PlayerProfile {
  return mergePlayerProfile(base, {
    playerId: ev.playerId,
    name: ev.name ?? null,
    heroId: ev.heroId ?? null,
    level: ev.level ?? null,
    seasonId: ev.seasonId ?? null,
    lastSeenAt: ev.ts,
  });
}

export type LogEvent =
  | BagModifyEvent
  | BagInitEvent
  | BagRemoveEvent
  | BagAddEvent
  | ItemChangeEvent
  | EnterLevelEvent
  | LevelPathEvent
  | LevelIdsEvent
  | PriceObservationEvent
  | PlayerInfoEvent;

/** Прирост или расход конкретного предмета, вычисленный из дельт слотов. */
export interface ItemDelta {
  ts: number;
  configBaseId: ConfigBaseId;
  /** Положительное — получено, отрицательное — израсходовано. */
  qty: number;
  proto: string;
  kind: ProtoKind;
  pageId: number;
  slotId: number;
}

/** Забег: отрезок между входом в зону и выходом из неё. */
export interface Run {
  areaId: number;
  asset: string | null;
  levelType: number | null;
  startedAt: number;
  endedAt: number | null;
  durationMs: number | null;
  isHub: boolean;
  /** Персонаж, которым бежали. null — лог старее этой фичи. */
  playerId: string | null;
  /**
   * Сезон (SeasonId из профиля), в котором бежали. Свой у каждого забега:
   * разбор логов двух сезонов разом иначе записывал все забеги в последний.
   * null — профиль в логе не встретился.
   */
  seasonId: number | null;
  drops: ItemDelta[];
  costs: ItemDelta[];
}

/** Счётчики для экрана самодиагностики. */
export interface Diagnostics {
  linesTotal: number;
  linesMatched: number;
  unknownProtos: Map<string, number>;
  /** Дельты, отброшенные из-за смены предмета в слоте. */
  slotRebases: number;
}
