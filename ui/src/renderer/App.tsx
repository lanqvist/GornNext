import { useEffect, useState } from 'react';

import { tr } from '../core/i18n.js';
import { RUN, pluralize } from '../core/plural.js';
import type { RetroScanProgress } from '../core/retroScan.js';
import type { Snapshot } from '../main/engine.js';
import { CurrentRun } from './components/CurrentRun.js';
import { Diagnostics } from './components/Diagnostics.js';
import { Header } from './components/Header.js';
import { Hero } from './components/Hero.js';
import { History } from './components/History.js';
import { Maps } from './components/Maps.js';
import { MissingPrices } from './components/MissingPrices.js';
import { Prices } from './components/Prices.js';
import { RecentRuns } from './components/RecentRuns.js';
import { SessionRecords } from './components/SessionRecords.js';
import { SettingsPanel } from './components/SettingsPanel.js';
import { Sidebar } from './components/Sidebar.js';
import { Strategies } from './components/Strategies.js';
import { UpdateBanner } from './components/UpdateBanner.js';
import { TiersContext, useTiers } from './tiers.js';

/**
 * Вкладки под сводкой. Раньше окно было лентой из 11 блоков — до карт
 * и истории приходилось листать. Сводка остаётся на виду, остальное —
 * в одном щелчке. Скрытая вкладка не рисуется вовсе.
 */
type Tab = 'session' | 'runs' | 'maps' | 'prices' | 'history';

const TABS: Array<[Tab, () => string]> = [
  ['session', () => tr('Сессия', 'Session')],
  ['runs', () => tr('Забеги', 'Runs')],
  ['maps', () => tr('Карты и стратегии', 'Maps & strategies')],
  ['prices', () => tr('Цены', 'Prices')],
  ['history', () => tr('История', 'History')],
];

const TAB_KEY = 'gorn.tab';

function savedTab(): Tab {
  try {
    const t = localStorage.getItem(TAB_KEY);
    return TABS.some(([k]) => k === t) ? (t as Tab) : 'session';
  } catch {
    return 'session';
  }
}

export function App() {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [scanProgress, setScanProgress] = useState<RetroScanProgress | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [tab, setTabState] = useState<Tab>(savedTab);
  const tiers = useTiers(snapshot?.jackpotAutoFe ?? 0);

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

  return (
    <TiersContext.Provider value={tiers}>
      <div className="app">
        <UpdateBanner />
        <Header snapshot={snapshot} onOpenSettings={() => setSettingsOpen(true)} />
        {settingsOpen && (
          <SettingsPanel minDropFe={snapshot.minDropFe} onClose={() => setSettingsOpen(false)}>
            {/* Диагностика — для тех, кто настраивает, а не для каждого взгляда. */}
            <Diagnostics snapshot={snapshot} />
          </SettingsPanel>
        )}
        <Hero snapshot={snapshot} />
        <SessionRecords snapshot={snapshot} />

        <nav className="tabs" role="tablist" aria-label={tr('Разделы', 'Sections')}>
          {TABS.map(([key, label]) => (
            <button
              key={key}
              role="tab"
              aria-selected={tab === key}
              className={tab === key ? 'tab on' : 'tab'}
              onClick={() => setTab(key)}
            >
              {label()}
              {key === 'prices' && snapshot.missing.length > 0 && (
                <span
                  className="tab-badge"
                  title={tr('Предметы без цены — оценка занижена', 'Items without a price — the estimate is low')}
                >
                  {snapshot.missing.length}
                </span>
              )}
            </button>
          ))}
        </nav>

        {tab === 'session' && (
          <div className="columns">
            <CurrentRun snapshot={snapshot} />
            <Sidebar snapshot={snapshot} />
          </div>
        )}
        {tab === 'runs' && <RecentRuns snapshot={snapshot} />}
        {tab === 'maps' && (
          <>
            <Strategies snapshot={snapshot} />
            <Maps snapshot={snapshot} />
          </>
        )}
        {tab === 'prices' && (
          <>
            <MissingPrices snapshot={snapshot} />
            <Prices snapshot={snapshot} />
          </>
        )}
        {tab === 'history' && <History snapshot={snapshot} />}
      </div>
    </TiersContext.Provider>
  );
}
