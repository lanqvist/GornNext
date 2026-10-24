/**
 * Мост `window.gorn` поверх Tauri — замена preload из Electron с тем же
 * интерфейсом (global.d.ts). Вызовы идут одной командой `gorn` с именем
 * канала, как у ipcMain эталона ('snapshot:get', 'session:reset', …);
 * подписки — событиями Tauri с теми же именами.
 *
 * Подключается первым импортом каждой страницы: страницы проверяют
 * `window.gorn` при загрузке (без него показываются демо-данные).
 */

interface TauriGlobal {
  core: { invoke: (cmd: string, args?: Record<string, unknown>) => Promise<unknown> };
  event: { listen: (event: string, cb: (e: { payload: unknown }) => void) => Promise<() => void> };
  window: { getCurrentWindow: () => { startDragging: () => Promise<void> } };
  webviewWindow: {
    getCurrentWebviewWindow: () => {
      listen: (event: string, cb: (e: { payload: unknown }) => void) => Promise<() => void>;
    };
  };
}

const tauri = (window as unknown as { __TAURI__?: TauriGlobal }).__TAURI__;

if (tauri) {
  const call = <T = unknown>(channel: string, ...args: unknown[]): Promise<T> =>
    tauri.core.invoke('gorn', { channel, args }) as Promise<T>;
  /** Сообщение без ответа: ошибка не должна всплывать в интерфейс. */
  const send = (channel: string, ...args: unknown[]): void => {
    call(channel, ...args).catch(() => {});
  };
  const self = tauri.webviewWindow.getCurrentWebviewWindow();
  /** Подписка; отписка — синхронно, даже пока слушатель ещё регистрируется. */
  const on = <T>(event: string, cb: (payload: T) => void): (() => void) => {
    let off: (() => void) | null = null;
    let cancelled = false;
    // Слушатель окна, а не глобальный: срез, адресованный другому окну (урезанный —
    // слою плашек), иначе приходил бы и сюда и ронял страницу.
    void self.listen(event, (e) => cb(e.payload as T)).then((un) => {
      if (cancelled) un();
      else off = un;
    });
    return () => {
      cancelled = true;
      off?.();
    };
  };

  const gorn: GornApi = {
    getSnapshot: () => call('snapshot:get'),
    onSnapshot: (cb) => on('snapshot', cb),
    onScanProgress: (cb) => on('scan:progress', cb),
    session: { reset: () => call('session:reset') },
    runs: {
      query: (query) => call('runs:query', query),
      exportCsv: () => call('runs:exportCsv'),
    },
    prices: {
      query: (query) => call('prices:query', query),
      setExcluded: (id, excluded) => call('items:setExcluded', id, excluded),
    },
    bag: { query: () => call('bag:query') },
    setMinDropFe: (value) => call('settings:setMinDropFe', value),
    getUiLanguage: () => call('settings:uiLanguage'),
    setUiLanguage: (language) => call('settings:setUiLanguage', language),
    onUiLanguage: (cb) => on('ui:language', cb),
    setRunScope: (scope) => call('settings:setRunScope', scope),
    update: {
      get: () => call('update:get'),
      check: () => call('update:check'),
      download: () => call('update:download'),
      install: () => send('update:install'),
      onState: (cb) => on('update:state', cb),
    },
    settings: {
      get: () => call('app:settings'),
      setCloseToTray: (enabled) => call('app:setCloseToTray', enabled),
      setAutoStart: (enabled) => call('app:setAutoStart', enabled),
      chooseLogDir: () => call('app:chooseLogDir'),
      resetLogDir: () => call('app:resetLogDir'),
      relaunch: () => send('app:relaunch'),
    },
    getItemNames: () => call('settings:itemNames'),
    setItemNames: (language) => call('settings:setItemNames', language),
    getCommunityPrices: () => call('settings:communityPrices'),
    setCommunityPrices: (enabled) => call('settings:setCommunityPrices', enabled),
    getPrisonHints: () => call('settings:prisonHints'),
    setPrisonHints: (enabled) => call('settings:setPrisonHints', enabled),
    getPrisonSaveShots: () => call('settings:prisonSaveShots'),
    setPrisonSaveShots: (enabled) => call('settings:setPrisonSaveShots', enabled),
    openPrisonShots: () => call('settings:openPrisonShots'),
    getPrisonFacts: () => call('settings:prisonFacts'),
    prison: {
      ready: () => send('prison:ready'),
      onHint: (cb) => on('prison:hint', cb),
      onSettled: (cb) => on('prison:settled', cb),
    },
    // Захват экрана в Tauri — нативный (Windows.Graphics.Capture), окна захвата нет.
    capture: {
      onStart: () => () => {},
      onStop: () => () => {},
      onBoost: () => () => {},
      onGrab: () => () => {},
      started: () => {},
      frame: () => {},
    },
    panel: {
      onOpen: (cb) => on('panel:open', cb),
      onHidden: (cb) => on('panel:hidden', cb),
      ready: () => send('panel:ready'),
      close: () => send('panel:close'),
      setTab: (tab) => send('panel:tab', tab),
    },
    toasts: {
      reportHeight: (height) => send('toasts:height', height),
      reportActive: (active) => send('toasts:active', active),
    },
    overlay: {
      getState: () => call('overlay:state'),
      toggle: () => call('overlay:toggle'),
      close: () => send('overlay:close'),
      setMode: (mode) => send('overlay:setMode', mode),
      cycleMode: () => send('overlay:cycleMode'),
      toggleLock: () => send('overlay:toggleLock'),
      setOpacity: (value) => send('overlay:setOpacity', value),
      reportHeight: (height, width) => send('overlay:height', height, width ?? null),
      setPrefs: (prefs) => send('overlay:setPrefs', prefs),
      onMode: (cb) => on('overlay:mode', cb),
      onLocked: (cb) => on('overlay:locked', cb),
      onEnabled: (cb) => on('overlay:enabled', cb),
      onPrefs: (cb) => on('overlay:prefs', cb),
    },
  };
  window.gorn = gorn;

  // Ошибки страницы — в журнал приложения (gorn.log): инструментов
  // разработчика у релизной сборки нет.
  const report = (what: string) => send('log', what.slice(0, 2000));
  window.addEventListener('error', (e) => report(`ошибка: ${e.message} (${e.filename}:${e.lineno})`));
  window.addEventListener('unhandledrejection', (e) => report(`отказ промиса: ${String((e.reason as Error)?.stack ?? e.reason)}`));
  report(`страница загружена: ${location.pathname}`);

  // Перетаскивание окна за области `-webkit-app-region: drag` (полоска оверлея):
  // в Electron это делает сам Chromium, WebView2 в Tauri — нет.
  const region = (el: Element): string => {
    const cs = getComputedStyle(el) as CSSStyleDeclaration & { webkitAppRegion?: string; appRegion?: string };
    const v = cs.webkitAppRegion || cs.appRegion || cs.getPropertyValue('-webkit-app-region') || cs.getPropertyValue('app-region');
    if (v) return v;
    // Свойство не отдаётся в вычисленном стиле — по разметке оверлея.
    if (el.classList.contains('ov-actions')) return 'no-drag';
    if (el.classList.contains('ov-grip')) return 'drag';
    return '';
  };
  document.addEventListener('mousedown', (e) => {
    if (e.button !== 0) return;
    for (let el = e.target instanceof Element ? e.target : null; el; el = el.parentElement) {
      if (/^(BUTTON|INPUT|SELECT|TEXTAREA|A)$/.test(el.tagName)) return;
      const r = region(el);
      if (r === 'no-drag') return;
      if (r === 'drag') {
        e.preventDefault();
        void tauri.window.getCurrentWindow().startDragging();
        return;
      }
    }
  });
}

export {};
