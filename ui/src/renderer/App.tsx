import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties } from 'react';

import { tr } from '../core/i18n.js';
import { RUN, pluralize } from '../core/plural.js';
import type { RetroScanProgress } from '../core/retroScan.js';
import type { Snapshot } from '../main/engine.js';
import { CurrentRun } from './components/CurrentRun.js';
import { Diagnostics } from './components/Diagnostics.js';
import { Header } from './components/Header.js';
import { Hero, sessionDelta } from './components/Hero.js';
import { Journal } from './components/Journal.js';
import { Maps } from './components/Maps.js';
import { MissingPrices } from './components/MissingPrices.js';
import { PricesDialog } from './components/Prices.js';
import { SettingsPanel } from './components/SettingsPanel.js';
import { Sidebar } from './components/Sidebar.js';
import { Strategies } from './components/Strategies.js';
import { UpdateBanner } from './components/UpdateBanner.js';
import { heatOf } from './overlay/heat.js';
import { TiersContext, useTiers } from './tiers.js';

/**
 * Вкладки под сводкой: сейчас, что было, куда идти. Скрытая вкладка не
 * рисуется вовсе. Было пять: «Забеги» и «История» слились в «Журнал»,
 * «Цены» ушли строкой под сводку и таблицей в Настройки → Данные.
 */
type Tab = 'session' | 'journal' | 'maps';

const TABS: Array<[Tab, () => string]> = [
  ['session', () => tr('Сессия', 'Session')],
  ['journal', () => tr('Журнал', 'Journal')],
  ['maps', () => tr('Карты', 'Maps')],
];

const TAB_KEY = 'gorn.tab';

/** Вкладки прошлых версий — куда они переехали. */
const MOVED: Record<string, Tab> = { runs: 'journal', history: 'journal', prices: 'session' };

function savedTab(): Tab {
  try {
    const t = localStorage.getItem(TAB_KEY) ?? '';
    if (TABS.some(([k]) => k === t)) return t as Tab;
    return MOVED[t] ?? 'session';
  } catch {
    return 'session';
  }
}

export function App() {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [scanProgress, setScanProgress] = useState<RetroScanProgress | null>(null);
  // Стенд: `?settings` открывает панель сразу — её вёрстку снимают без кликов.
  // Стенд: `?prices` — сразу окно цен, как `?settings` — панель настроек.
  const [pricesOpen, setPricesOpen] = useState<boolean>(
    () => import.meta.env.DEV && !window.gorn && new URLSearchParams(location.search).has('prices'),
  );
  const [settingsOpen, setSettingsOpen] = useState<boolean>(
    () => import.meta.env.DEV && !window.gorn && new URLSearchParams(location.search).has('settings'),
  );
  const [tab, setTabState] = useState<Tab>(savedTab);
  const tiers = useTiers(snapshot?.jackpotAutoFe ?? 0);
  const tabsRef = useRef<HTMLElement>(null);
  const [ink, setInk] = useState<{ left: number; width: number } | null>(null);

  // Подчёркивание вкладки скользит к выбранной: меряем её положение
  // при смене вкладки и когда полоса вкладок меняет размер.
  useLayoutEffect(() => {
    const nav = tabsRef.current;
    if (!nav) return;
    const measure = () => {
      const on = nav.querySelector<HTMLElement>('.tab.on');
      if (on) setInk({ left: on.offsetLeft, width: on.offsetWidth });
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(nav);
    return () => ro.disconnect();
  }, [tab, snapshot !== null]);

  const setTab = (t: Tab) => {
    setTabState(t);
    try {
      localStorage.setItem(TAB_KEY, t);
    } catch {
      // Без хранилища вкладка просто не запомнится.
    }
  };

  useEffect(() => {
    // В браузере main-процесса нет — показываем демо-данные, чтобы вёрстку
    // можно было проверять без запуска Electron.
    if (!window.gorn) {
      if (import.meta.env.DEV) {
        void import('./demo.js').then((m) => setSnapshot(m.demoSnapshot));
      }
      return;
    }
    void window.gorn.getSnapshot().then((s) => s && setSnapshot(s));
    const offSnapshot = window.gorn.onSnapshot(setSnapshot);
    const offScan = window.gorn.onScanProgress(setScanProgress);
    return () => {
      offSnapshot();
      offScan();
    };
  }, []);

  // Секундного таймера здесь нет: время спрашивают сами часы забега и
  // возраст лога (useNow), иначе каждую секунду перерисовывалось бы всё окно.

  if (!snapshot) {
    return (
      <div className="boot">
        <div className="boot-spark" />
        <p>{tr('Разбираю логи…', 'Reading logs…')}</p>
        {/* На большой пачке логов без живой цифры заглушка неотличима
            от зависания. */}
        {scanProgress && (
          <p className="muted small">
            {tr(
              `Файл ${scanProgress.index + 1} из ${scanProgress.total}`,
              `File ${scanProgress.index + 1} of ${scanProgress.total}`,
            )}
            {scanProgress.runsFound > 0 && (
              <>
                {' '}
                · {tr('найдено', 'found')} {pluralize(scanProgress.runsFound, RUN)}
              </>
            )}
          </p>
        )}
      </div>
    );
  }

  // Окно калится темпом карт сессии: кромка сверху, жар под главной цифрой,
  // подчёркивание вкладки. Цвет перетекает (@property в styles.css).
  const delta = sessionDelta(snapshot);
  const heat = delta === null ? null : heatOf(delta);

  return (
    <TiersContext.Provider value={tiers}>
      <div
        className={`app ${heat ? 'app-hot' : ''}`}
        style={heat ? ({ '--heat': heat.color, '--glow': heat.glow } as CSSProperties) : undefined}
      >
        <i className="app-edge" aria-hidden="true" />
        <UpdateBanner />
        <Header snapshot={snapshot} onOpenSettings={() => setSettingsOpen(true)} />
        {settingsOpen && (
          <SettingsPanel
            minDropFe={snapshot.minDropFe}
            onClose={() => setSettingsOpen(false)}
            onOpenPrices={() => setPricesOpen(true)}
          >
            {/* Диагностика — для тех, кто настраивает, а не для каждого взгляда. */}
            <Diagnostics snapshot={snapshot} />
          </SettingsPanel>
        )}
        {pricesOpen && <PricesDialog snapshot={snapshot} onClose={() => setPricesOpen(false)} />}
        <Hero snapshot={snapshot} />
        <MissingPrices snapshot={snapshot} />

        <nav className="tabs" role="tablist" aria-label={tr('Разделы', 'Sections')} ref={tabsRef}>
          {TABS.map(([key, label]) => (
            <button
              key={key}
              role="tab"
              aria-selected={tab === key}
              className={tab === key ? 'tab on' : 'tab'}
              onClick={() => setTab(key)}
            >
              {label()}
            </button>
          ))}
          {ink && <i className="tabs-ink" style={{ left: ink.left, width: ink.width }} />}
        </nav>

        {tab === 'session' && (
          <div className="columns">
            <CurrentRun snapshot={snapshot} />
            <Sidebar snapshot={snapshot} />
          </div>
        )}
        {tab === 'journal' && <Journal snapshot={snapshot} />}
        {tab === 'maps' && (
          <>
            <Strategies snapshot={snapshot} />
            <Maps snapshot={snapshot} />
          </>
        )}
      </div>
    </TiersContext.Provider>
  );
}
