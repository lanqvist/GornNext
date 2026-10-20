import { memo, useEffect, useRef, useState } from 'react';

import { tr } from '../core/i18n.js';
import { RUN, VISIT, pluralize } from '../core/plural.js';
import type { DropView, PriceRow, RunView, Snapshot } from '../main/engine.js';
import { Fe, FeIcon } from './components/Fe.js';
import { ItemIcon } from './components/ui.js';
import { ago, clock, copyText, duration, num } from './format.js';
import { useNow } from './useNow.js';

type Tab = GornPanelTab;

const TABS: Array<[Tab, () => string]> = [
  ['run', () => tr('Забег', 'Run')],
  ['session', () => tr('Сессия', 'Session')],
  ['prices', () => tr('Цены', 'Prices')],
  ['maps', () => tr('Карты', 'Maps')],
];

/** Пороги ступеней ценности — те же, что в оверлее (явные из настроек либо авто). */
interface Tiers {
  gold: number;
  orange: number;
}

const tierOf = (fe: number | null, t: Tiers) =>
  fe === null ? 'none' : fe >= t.gold ? 'gold' : fe >= t.orange ? 'orange' : 'plain';

/**
 * Пульт поверх игры — второй слой оверлея, как в Awakened PoE Trade.
 * Открывается клавишей (Ctrl+Alt+T, к ценам — Ctrl+Alt+F), принимает мышь
 * и клавиатуру, Esc возвращает в игру.
 *
 * Спрятанный пульт не рисует ничего: дерево снимается целиком, таймеры
 * останавливаются, снимки ему не шлются (main/panel.ts).
 */
export function PanelApp() {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [visible, setVisible] = useState(false);
  const [tab, setTabState] = useState<Tab>('run');
  const [scale, setScale] = useState(1);
  // Каждое открытие — новый номер: по нему поиск цены снова берёт фокус.
  const [openSeq, setOpenSeq] = useState(0);
  const [tiers, setTiers] = useState<Tiers | null>(null);

  const setTab = (t: Tab) => {
    setTabState(t);
    window.gorn?.panel.setTab(t);
  };

  useEffect(() => {
    if (!window.gorn) {
      // Браузерное демо: ?tab=prices&lang=en.
      if (import.meta.env.DEV) {
        const t = new URLSearchParams(location.search).get('tab');
        if (TABS.some(([k]) => k === t)) setTabState(t as Tab);
        setVisible(true);
        void import('./demo.js').then((m) => setSnapshot(m.demoSnapshot));
      }
      return;
    }
    void window.gorn.getSnapshot().then((s) => s && setSnapshot(s));
    const offSnapshot = window.gorn.onSnapshot(setSnapshot);
    const offOpen = window.gorn.panel.onOpen(({ tab: t, scale: s }) => {
      if (t) setTabState(t);
      setScale(s);
      setVisible(true);
      setOpenSeq((n) => n + 1);
      // Пороги «золота» могли поменять в настройках оверлея — берём при открытии.
      void window.gorn.overlay.getState().then((st) =>
        setTiers(st ? { gold: st.jackpotFe, orange: st.exaltedFe } : null),
      );
    });
    const offHidden = window.gorn.panel.onHidden(() => setVisible(false));
    // Только теперь main может показывать пульт: раньше «открыть» ушло бы в пустоту.
    window.gorn.panel.ready();
    return () => {
      offSnapshot();
      offOpen();
      offHidden();
    };
  }, []);

  useEffect(() => {
    if (!visible) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        window.gorn?.panel.close();
        return;
      }
      // Цифры — вкладки, пока не печатают в поиск.
      const typing = e.target instanceof HTMLInputElement;
      const n = Number(e.key);
      if (!typing && !e.ctrlKey && !e.altKey && n >= 1 && n <= TABS.length) {
        e.preventDefault();
        setTab(TABS[n - 1]![0]);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [visible]);

  if (!visible) return null;

  const goldAuto = snapshot?.jackpotAutoFe ?? 50;
  const gold = tiers && tiers.gold > 0 ? tiers.gold : goldAuto;
  const t: Tiers = { gold, orange: tiers && tiers.orange > 0 ? tiers.orange : gold / 5 };

  return (
    <div className="pn" style={{ zoom: scale }}>
      <header className="pn-top">
        <span className="pn-brand">
          <FeIcon />
          {tr('Горн', 'Gorn')}
        </span>
        <nav className="pn-tabs" role="tablist">
          {TABS.map(([key, label], i) => (
            <button
              key={key}
              role="tab"
              aria-selected={tab === key}
              className={tab === key ? 'on' : ''}
              onClick={() => setTab(key)}
            >
              {label()}
              <kbd>{i + 1}</kbd>
            </button>
          ))}
        </nav>
        <button className="pn-close" onClick={() => window.gorn?.panel.close()}>
          {tr('в игру', 'back to game')} <kbd>Esc</kbd>
        </button>
      </header>

      <main className="pn-body">
        {!snapshot ? (
          <p className="pn-empty">{tr('Разбираю логи…', 'Reading logs…')}</p>
        ) : tab === 'run' ? (
          <RunTab snapshot={snapshot} tiers={t} />
        ) : tab === 'session' ? (
          <SessionTab snapshot={snapshot} />
        ) : tab === 'prices' ? (
          <PricesTab snapshot={snapshot} openSeq={openSeq} />
        ) : (
          <MapsTab snapshot={snapshot} />
        )}
      </main>

      <footer className="pn-foot">
        <span>
          <kbd>Ctrl+Alt+T</kbd> {tr('пульт', 'panel')}
        </span>
        <span>
          <kbd>Ctrl+Alt+F</kbd> {tr('цены', 'prices')}
        </span>
        <span>
          <kbd>1</kbd>–<kbd>4</kbd> {tr('вкладки', 'tabs')}
        </span>
        <span>
          <kbd>Esc</kbd> {tr('в игру', 'back to game')}
        </span>
      </footer>
    </div>
  );
}

/* ── Забег ───────────────────────────────────────────────────── */

/** Часы открытой карты — единственное, что тикает в пульте. */
function RunClock({ startedAt }: { startedAt: number }) {
  const now = useNow(1000);
  return <>{clock(now - startedAt)}</>;
}

const RunTab = memo(function RunTab({ snapshot, tiers }: { snapshot: Snapshot; tiers: Tiers }) {
  const run = snapshot.current ?? snapshot.recent[0] ?? null;
  if (!run) {
    return <p className="pn-empty">{tr('Забегов ещё нет.', 'No runs yet.')}</p>;
  }
  const live = snapshot.current !== null;
  // К норме — медиане последних 20 забегов, как везде.
  const norm = snapshot.norm;
  const delta = norm > 0 ? Math.round(((run.perHour - norm) / norm) * 100) : null;
  const previous = live ? snapshot.recent[0] ?? null : snapshot.recent[1] ?? null;

  return (
    <div className="pn-cols">
      <section className="pn-col pn-wide">
        <div className="pn-run-head">
          <span className="pn-chip">
            {live ? <RunClock startedAt={run.startedAt} /> : clock(run.durationMs ?? 0)}
          </span>
          <span className="pn-run-name">
            {live ? run.name : tr(`прошлый: ${run.name}`, `last: ${run.name}`)}
          </span>
        </div>
        <div className="pn-hero">
          <Fe value={run.netFe} signed />
          {delta !== null && (
            <span
              className={`pn-delta ${delta >= 0 ? 'up' : 'down'}`}
              title={tr('Темп карты к норме — медиане последних 20 забегов', 'Map pace vs the norm — the median of the last 20 runs')}
            >
              {delta >= 0 ? '↑' : '↓'} {Math.abs(delta)}%
            </span>
          )}
          <span className="pn-sub">
            {num(run.perHour)} {tr('в час', 'per hour')}
          </span>
        </div>
        <DropList drops={run.drops} snapshot={snapshot} tiers={tiers} />
      </section>

      <section className="pn-col">
        <h3>{tr('Вложено', 'Spent')}</h3>
        {run.costs.length === 0 ? (
          <p className="pn-muted">{tr('Без вложений.', 'Nothing spent.')}</p>
        ) : (
          <ul className="pn-list">
            {run.costs.map((d) => (
              <li key={d.configBaseId}>
                <span className="pn-ico">
                  <ItemIcon src={d.icon} />
                </span>
                <span className="pn-name">{d.name}</span>
                <span className="pn-qty">×{num(d.qty)}</span>
                <span className="pn-fe">{d.totalFe === null ? '—' : <Fe value={d.totalFe} />}</span>
              </li>
            ))}
          </ul>
        )}
        <div className="pn-kv">
          <span>{tr('добыча', 'loot')}</span>
          <Fe value={run.valueFe} />
        </div>
        <div className="pn-kv">
          <span>{tr('вложено', 'spent')}</span>
          <Fe value={run.costFe} />
        </div>
        {previous && (
          <>
            <h3>{tr('До этого', 'Before')}</h3>
            <RunLine run={previous} norm={snapshot.norm} />
          </>
        )}
      </section>
    </div>
  );
});

function DropList({
  drops,
  snapshot,
  tiers,
}: {
  drops: DropView[];
  snapshot: Snapshot;
  tiers: Tiers;
}) {
  const shown = drops.filter(
    (d) => d.priceSource !== 'excluded' && !(d.totalFe !== null && d.totalFe < snapshot.minDropFe),
  );
  if (shown.length === 0) {
    return <p className="pn-muted">{tr('Пока ничего не подобрано.', 'Nothing picked up yet.')}</p>;
  }
  const max = Math.max(1, ...shown.map((d) => d.totalFe ?? 0));
  return (
    <ul className="pn-drops">
      {shown.map((d) => (
        <li key={d.configBaseId} className={`t-${tierOf(d.totalFe, tiers)}`}>
          <i style={{ width: `${d.totalFe === null ? 0 : (d.totalFe / max) * 100}%` }} />
          <span className="pn-ico">
            <ItemIcon src={d.icon} />
          </span>
          <span className="pn-name">{d.name}</span>
          <span className="pn-qty">{d.qty > 1 ? `×${num(d.qty)}` : ''}</span>
          <span className="pn-fe">
            {d.totalFe === null ? tr('без цены', 'no price') : <Fe value={d.totalFe} />}
          </span>
        </li>
      ))}
    </ul>
  );
}

function RunLine({ run, norm }: { run: RunView; norm: number }) {
  const delta = norm > 0 ? Math.round(((run.perHour - norm) / norm) * 100) : null;
  return (
    <div className="pn-runline">
      <span className="pn-name">
        {run.name}
        {run.durationMs != null && ` · ${clock(run.durationMs)}`}
      </span>
      <Fe value={run.netFe} signed />
      {delta !== null && (
        <span className={delta >= 0 ? 'up' : 'down'}>
          {delta >= 0 ? '+' : '−'}
          {Math.abs(delta)}%
        </span>
      )}
    </div>
  );
}

/* ── Сессия ──────────────────────────────────────────────────── */

const SessionTab = memo(function SessionTab({ snapshot }: { snapshot: Snapshot }) {
  const net = snapshot.totals.valueFe - snapshot.totals.costFe;
  const now = useNow(60_000);
  return (
    <div className="pn-cols">
      <section className="pn-col pn-wide">
        <div className="pn-tiles">
          <div>
            <small>{tr('сессия', 'session')}</small>
            <b className="pn-green">
              <Fe value={net} signed />
            </b>
          </div>
          <div>
            <small>{tr('в час', 'per hour')}</small>
            <b>{num(snapshot.liveRate)}</b>
          </div>
          <div>
            <small>{tr('медиана, /ч', 'median, /h')}</small>
            <b>{num(snapshot.rate.median)}</b>
          </div>
          <div>
            <small>{tr('на картах', 'in maps')}</small>
            <b>{duration(snapshot.totals.inMapMs)}</b>
          </div>
        </div>
        {snapshot.prison.runs > 0 && (
          <p className="pn-muted">
            {tr('тюрьма', 'prison')}: <Fe value={snapshot.prison.netFe} signed /> {tr('за', 'in')}{' '}
            {pluralize(snapshot.prison.runs, VISIT)} —{' '}
            {tr('в итоге сессии есть, в темпе нет', 'in the session total, not in the pace')}
          </p>
        )}
        <h3>
          {tr('Последние забеги', 'Recent runs')} ·{' '}
          <span className="pn-muted">{pluralize(snapshot.totals.runs, RUN)}</span>
        </h3>
        <ul className="pn-runs">
          {snapshot.recent.slice(0, 9).map((r) => (
            <li key={r.key}>
              <RunLine run={r} norm={snapshot.norm} />
            </li>
          ))}
        </ul>
      </section>
      <section className="pn-col">
        <h3>{tr('Крупное за сессию', 'Top drops this session')}</h3>
        {snapshot.sessionTop.length === 0 ? (
          <p className="pn-muted">{tr('Пока нечем похвастаться.', 'Nothing to brag about yet.')}</p>
        ) : (
          <ul className="pn-finds">
            {snapshot.sessionTop.map((d) => (
              <li key={`${d.configBaseId}:${d.at}`}>
                <span className="pn-ico">
                  <ItemIcon src={d.icon} />
                </span>
                <span className="pn-find-text">
                  <span className="pn-name">
                    {d.name}
                    {d.qty > 1 && ` ×${num(d.qty)}`}
                  </span>
                  <small>
                    {d.runName} · {ago(now - d.at)}
                  </small>
                </span>
                <span className="pn-fe">
                  <Fe value={d.totalFe} />
                </span>
              </li>
            ))}
          </ul>
        )}
        {snapshot.bag && (
          <>
            <h3>{tr('Сумка', 'Bag')}</h3>
            <div className="pn-kv">
              <span>{tr('оценка', 'value')}</span>
              <Fe value={snapshot.bag.valueFe} />
            </div>
          </>
        )}
      </section>
    </div>
  );
});

/* ── Цены ────────────────────────────────────────────────────── */

function PriceSource({ row }: { row: PriceRow }) {
  if (row.source === 'excluded') {
    return <span className="pn-src">{tr('не учитывается', 'ignored')}</span>;
  }
  if (row.source === 'community') {
    return <span className="pn-src com">{tr('сообщество', 'community')}</span>;
  }
  if (row.ownObservedAt !== null) {
    return (
      <span className="pn-src own">
        {tr('своя', 'own')} · {ago(Date.now() - row.ownObservedAt)}
      </span>
    );
  }
  return <span className="pn-src">—</span>;
}

function PricesTab({ snapshot, openSeq }: { snapshot: Snapshot; openSeq: number }) {
  const [query, setQuery] = useState('');
  const [rows, setRows] = useState<PriceRow[]>([]);
  const [sel, setSel] = useState(0);
  const [copied, setCopied] = useState<string | null>(null);
  const input = useRef<HTMLInputElement>(null);

  // Каждое открытие пульта — сразу печатать: фокус в поиск, прошлый запрос выделен.
  useEffect(() => {
    input.current?.focus();
    input.current?.select();
  }, [openSeq]);

  const priceKey = `${snapshot.diagnostics.pricedItems}:${snapshot.diagnostics.communityPrices}`;
  useEffect(() => {
    const timer = setTimeout(
      () => {
        const q = query.trim();
        if (window.gorn) void window.gorn.prices.query(q).then(setRows);
        else if (import.meta.env.DEV) {
          void import('./demo.js').then((m) =>
            setRows(
              m.demoPrices.filter((r) =>
                r.name.toLocaleLowerCase('ru').includes(q.toLocaleLowerCase('ru')),
              ),
            ),
          );
        }
        setSel(0);
      },
      query === '' ? 0 : 150,
    );
    return () => clearTimeout(timer);
  }, [query, priceKey]);

  const copy = (name: string) => {
    void copyText(name);
    setCopied(name);
  };

  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      setSel((i) => Math.min(rows.length - 1, i + 1));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setSel((i) => Math.max(0, i - 1));
    } else if (e.key === 'Enter' && rows[sel]) {
      e.preventDefault();
      copy(rows[sel]!.name);
    }
  };

  const shown = rows.slice(0, 12);

  return (
    <div className="pn-cols">
      <section className="pn-col pn-wide">
        <label className="pn-search">
          <span className="pn-search-ico" aria-hidden="true">
            ⌕
          </span>
          <input
            ref={input}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={onKey}
            placeholder={tr('Название предмета', 'Item name')}
            spellCheck={false}
          />
          <span className="pn-muted">
            {rows.length > shown.length ? `${shown.length} / ${rows.length}` : rows.length}
          </span>
        </label>
        <ul className="pn-prices" role="listbox">
          {shown.map((r, i) => (
            <li
              key={r.configBaseId}
              role="option"
              aria-selected={i === sel}
              className={i === sel ? 'sel' : ''}
              onMouseEnter={() => setSel(i)}
              onClick={() => copy(r.name)}
              title={tr('Скопировать название', 'Copy the name')}
            >
              <span className="pn-ico">
                <ItemIcon src={r.icon} />
              </span>
              <span className="pn-name">{r.name}</span>
              <PriceSource row={r} />
              <span className="pn-fe">{r.fe === null ? '—' : <Fe value={r.fe} />}</span>
            </li>
          ))}
          {shown.length === 0 && (
            <li className="pn-muted">{tr('Ничего не нашлось.', 'Nothing found.')}</li>
          )}
        </ul>
        <p className="pn-hint">
          {copied
            ? tr(
                `«${copied}» скопировано — вставьте в поиск торгового дома. Пробивка там обновит цену.`,
                `“${copied}” copied — paste it into the trade house search. Checking it there updates the price.`,
              )
            : tr(
                '↑ ↓ — выбрать, Enter или щелчок — скопировать название для торгового дома.',
                '↑ ↓ to pick, Enter or click to copy the name for the trade house.',
              )}
        </p>
      </section>
      <section className="pn-col">
        <h3>{tr('Без цены — занижают доход', 'No price — understate income')}</h3>
        {snapshot.missing.length === 0 ? (
          <p className="pn-muted">{tr('Всё оценено.', 'Everything is priced.')}</p>
        ) : (
          <ul className="pn-missing">
            {snapshot.missing.slice(0, 6).map((m) => (
              <li key={m.configBaseId}>
                <span className="pn-ico">
                  <ItemIcon src={m.icon} />
                </span>
                <span className="pn-name">{m.name}</span>
                <small>
                  {num(m.qty)} {tr('шт', 'pcs')} · {pluralize(m.runs, RUN)}
                  {m.isCost && ` · ${tr('вложение', 'spent on maps')}`}
                </small>
                <span className="pn-btns">
                  <button className="pri" onClick={() => copy(m.name)}>
                    {tr('Скопировать', 'Copy')}
                  </button>
                  <button onClick={() => void window.gorn?.prices.setExcluded(m.configBaseId, true)}>
                    {tr('Не учитывать', 'Ignore')}
                  </button>
                </span>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}

/* ── Карты ───────────────────────────────────────────────────── */

const MapsTab = memo(function MapsTab({ snapshot }: { snapshot: Snapshot }) {
  if (snapshot.maps.length === 0) {
    return <p className="pn-empty">{tr('Карт ещё нет.', 'No maps yet.')}</p>;
  }
  const best = Math.max(...snapshot.maps.filter((m) => m.runs >= 5).map((m) => m.rate.median));
  const max = Math.max(1, ...snapshot.maps.map((m) => m.rate.median));
  return (
    <table className="pn-maps">
      <thead>
        <tr>
          <th>{tr('Карта', 'Map')}</th>
          <th className="num">{tr('Забегов', 'Runs')}</th>
          <th>{tr('В час, медиана', 'Per hour, median')}</th>
          <th className="num">{tr('За карту', 'Per map')}</th>
          <th className="num">{tr('Время', 'Time')}</th>
        </tr>
      </thead>
      <tbody>
        {snapshot.maps.map((m) => (
          <tr key={m.name} className={m.runs >= 5 && m.rate.median === best ? 'best' : ''}>
            <td>{m.name}</td>
            <td className="num">{m.runs}</td>
            <td>
              <span className="pn-bar">
                <i style={{ width: `${(Math.max(0, m.rate.median) / max) * 100}%` }} />
                <b>{num(m.rate.median)}</b>
              </span>
            </td>
            <td className="num">
              <Fe value={m.netPerRun} />
            </td>
            <td className="num">{duration(m.avgActiveMs)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
});
