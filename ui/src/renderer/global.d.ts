import type { UiLanguage } from '../core/i18n.js';
import type { RetroScanProgress } from '../core/retroScan.js';
import type { BagGrid, PriceRow, RunQueryResult, Snapshot } from '../main/engine.js';
import type { OverlayMode } from '../main/settings.js';
import type { UpdateState } from '../main/updater.js';

/**
 * Единственное объявление моста в renderer. Раньше оно дублировалось
 * в дашборде и в оверлее — два несовпадающих описания одного и того же
 * глобального объекта рано или поздно расходятся.
 */
declare global {
  interface GornOverlayPrefs {
    popups: boolean;
    sound: boolean;
    volume: number;
    divineFe: number;
    jackpotFe: number;
    exaltedFe: number;
    fontScale: number;
    goalFe: number;
    /** Жар темпа: кромка оверлея цветом каления по темпу карты к норме, искры выше нормы. */
    heat: boolean;
  }

  interface GornOverlayApi {
    getState: () => Promise<{
      enabled: boolean;
      mode: OverlayMode;
      locked: boolean;
      opacity: number;
      fontScale: number;
      popups: boolean;
      sound: boolean;
      volume: number;
      divineFe: number;
      jackpotFe: number;
      exaltedFe: number;
      goalFe: number;
      heat?: boolean;
    }>;
    toggle: () => Promise<boolean>;
    close: () => void;
    setMode: (mode: OverlayMode) => void;
    cycleMode: () => void;
    toggleLock: () => void;
    setOpacity: (value: number) => void;
    /** Высота по содержимому; ширина — только у ленты. */
    reportHeight: (height: number, width?: number) => void;
    setPrefs: (prefs: Partial<GornOverlayPrefs>) => void;
    onMode: (cb: (mode: OverlayMode) => void) => () => void;
    onLocked: (cb: (locked: boolean) => void) => () => void;
    onEnabled: (cb: (open: boolean) => void) => () => void;
    onPrefs: (cb: (prefs: GornOverlayPrefs) => void) => () => void;
  }

  type NameLanguage = 'ru' | 'en' | 'both';

  /** Вкладки пульта — как PanelTab в main/panel.ts. */
  type GornPanelTab = 'run' | 'session' | 'prices' | 'maps' | 'bag';

  interface GornApi {
    getSnapshot: () => Promise<Snapshot | null>;
    onSnapshot: (cb: (snapshot: Snapshot) => void) => () => void;
    onScanProgress: (cb: (progress: RetroScanProgress) => void) => () => void;
    getItemNames: () => Promise<NameLanguage>;
    setItemNames: (language: NameLanguage) => Promise<NameLanguage>;
    getCommunityPrices: () => Promise<boolean>;
    setCommunityPrices: (enabled: boolean) => Promise<boolean>;
    session: {
      reset: () => Promise<void>;
    };
    setRunScope: (scope: { allSeasons: boolean }) => Promise<void>;
    prices: {
      query: (query: string) => Promise<PriceRow[]>;
      setExcluded: (configBaseId: number, excluded: boolean) => Promise<number[]>;
    };
    bag: {
      query: () => Promise<BagGrid | null>;
    };
    setMinDropFe: (value: number) => Promise<number>;
    getUiLanguage: () => Promise<UiLanguage>;
    setUiLanguage: (language: UiLanguage) => Promise<UiLanguage>;
    onUiLanguage: (cb: (language: UiLanguage) => void) => () => void;
    runs: {
      query: (query: string) => Promise<RunQueryResult | null>;
      exportCsv: () => Promise<
        | { status: 'cancelled' }
        | { status: 'saved'; path: string }
        | { status: 'error'; message: string }
      >;
    };
    panel: {
      onOpen: (cb: (req: { tab: GornPanelTab | null; scale: number }) => void) => () => void;
      onHidden: (cb: () => void) => () => void;
      ready: () => void;
      close: () => void;
      setTab: (tab: GornPanelTab) => void;
    };
    toasts: {
      reportHeight: (height: number) => void;
      reportActive: (active: boolean) => void;
    };
    overlay: GornOverlayApi;
    update: {
      get: () => Promise<UpdateState | null>;
      check: () => Promise<void>;
      download: () => Promise<void>;
      install: () => void;
      onState: (cb: (state: UpdateState) => void) => () => void;
    };
    getPrisonHints: () => Promise<boolean>;
    setPrisonHints: (enabled: boolean) => Promise<boolean>;
    getPrisonSaveShots: () => Promise<boolean>;
    setPrisonSaveShots: (enabled: boolean) => Promise<boolean>;
    openPrisonShots: () => Promise<string>;
    /** Журнал советника тюрьмы: забегов с решениями и решений всего. */
    getPrisonFacts: () => Promise<{ runs: number; decisions: number }>;
    capture: {
      onStart: (cb: (req: { sourceId: string; width: number; height: number }) => void) => () => void;
      onStop: (cb: () => void) => () => void;
      onBoost: (cb: () => void) => () => void;
      onGrab: (cb: (id: number, stableMs: number, maxMs: number, okAgeMs: number) => void) => () => void;
      started: (err: string | null) => void;
      frame: (id: number, frame: { width: number; height: number; buffer: ArrayBuffer } | null, err: string | null) => void;
    };
    prison: {
      ready: () => void;
      onHint: (cb: (hint: GornPrisonHint | null) => void) => () => void;
      onSettled: (cb: (live: boolean) => void) => () => void;
    };
    settings: {
      get: () => Promise<GornAppSettings>;
      setCloseToTray: (enabled: boolean) => Promise<boolean>;
      setAutoStart: (enabled: boolean) => Promise<boolean>;
      chooseLogDir: () => Promise<GornLogDirResult>;
      resetLogDir: () => Promise<GornLogDirResult>;
      relaunch: () => void;
    };
  }

  /**
   * Подсказка в тюрьме в координатах окна стрелок (см. app/src/overlay.rs).
   * Шаг метки: 1 — жать сейчас, 2 — следующий клик того же хода, 0 — только показать.
   */
  interface GornPrisonHint {
    title: string;
    sub: string;
    /** Ход из двух кликов: на метках и плашке — номера шагов. */
    twoSteps: boolean;
    marks: Array<
      | { kind: 'arrow'; from: [number, number]; to: [number, number]; step: number }
      | { kind: 'ring'; at: [number, number]; r: number; step: number }
      | { kind: 'box'; box: [number, number, number, number]; step: number }
      | { kind: 'path'; points: Array<[number, number]> }
    >;
    /** Плашка: точка и чем она к ней пристёгнута; card — ширина карточки над/под плашкой. */
    plate: { at: [number, number]; anchor: 'top' | 'bottom' | 'center'; card?: number };
    live: boolean;
    phase: 'instant' | 'final';
    /** Ход сделан, новая подсказка считается. */
    thinking?: boolean;
  }

  interface GornAppSettings {
    /** Папка логов, указанная вручную; null — автопоиск. */
    logDir: string | null;
    /** Папка, с которой движок реально работает; null — игра не найдена. */
    resolvedLogDir: string | null;
    closeToTray: boolean;
    autoStart: boolean;
    version: string;
    /** Горячие клавиши, занятые другими программами. */
    busyHotkeys: string[];
  }

  interface GornLogDirResult {
    status: 'applied' | 'needsRestart' | 'invalid' | 'cancelled';
    dir?: string;
  }

  interface Window {
    /** Отсутствует в обычном браузере — там показываются демо-данные. */
    gorn: GornApi;
  }
}

export {};
