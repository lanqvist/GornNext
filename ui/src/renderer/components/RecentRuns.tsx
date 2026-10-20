import { useEffect, useState } from 'react';

import { tr } from '../../core/i18n.js';
import { RUN, pluralize } from '../../core/plural.js';
import type { RunQueryResult, RunView, Snapshot } from '../../main/engine.js';
import { cacheDate, duration, num, time } from '../format.js';
import { DropList } from './DropList.js';
import { Fe, FeIcon } from './Fe.js';
import { Tip } from './ui.js';

/**
 * Мини-распределение темпа по забегам: текст «разброс шире медианы»
 * становится картинкой. Пунктир — медиана; корзина с самым удачным
 * забегом горит акцентом, когда на нём держится заметная доля дохода.
 */
function RateHistogram({ snapshot }: { snapshot: Snapshot }) {
  const rates = snapshot.recent.map((r) => r.perHour);
  if (rates.length < 8) return null;

  const BIN_COUNT = 12;
  const max = Math.max(...rates);
  const min = Math.min(...rates, 0);
  const span = max - min || 1;
  const bins = new Array<number>(BIN_COUNT).fill(0);
  for (const v of rates) {
    const idx = Math.min(BIN_COUNT - 1, Math.floor(((v - min) / span) * BIN_COUNT));
    bins[idx] = (bins[idx] ?? 0) + 1;
  }
  const top = Math.max(...bins);
  const bestBin = Math.min(BIN_COUNT - 1, Math.floor(((max - min) / span) * BIN_COUNT));
  const highlightBest = snapshot.rate.topShare > 0.3;
  const medianX = ((snapshot.rate.median - min) / span) * 100;

  const w = 160;
  const h = 40;
  const gap = 2;
  const barW = (w - gap * (BIN_COUNT - 1)) / BIN_COUNT;

  return (
    <svg
      viewBox={`0 0 ${w} ${h}`}
      width={w}
      height={h}
      className="hist"
      role="img"
      aria-label={tr('Распределение темпа по забегам', 'Rate distribution across runs')}
    >
      {bins.map((count, i) => {
        const barH = count === 0 ? 0 : Math.max(3, (count / top) * (h - 4));
        const lo = min + (span * i) / BIN_COUNT;
        const hi = min + (span * (i + 1)) / BIN_COUNT;
        return (
          <rect
            key={i}
            x={i * (barW + gap)}
            y={h - barH}
            width={barW}
            height={barH}
            rx={1.5}
            className={
              highlightBest && i === bestBin && count > 0 ? 'hist-bar hist-bar-top' : 'hist-bar'
            }
          >
            <title>
              {tr(
                `${num(lo)}–${num(hi)} ФЕ/час: ${pluralize(count, RUN)}`,
                `${num(lo)}–${num(hi)} FE/h: ${pluralize(count, RUN)}`,
              )}
            </title>
          </rect>
        );
      })}
      <line
        x1={`${medianX}%`}
        x2={`${medianX}%`}
        y1={0}
        y2={h}
        className="hist-median"
      >
        <title>
          {tr(
            `медиана ${num(snapshot.rate.median)} ФЕ/час`,
            `median ${num(snapshot.rate.median)} FE/h`,
          )}
        </title>
      </line>
    </svg>
  );
}

/**
 * Поиск в браузерном демо: main-процесса нет, ищем по забегам из среза.
 * В приложении поиск идёт по всей истории в main.
 */
function localQuery(runs: RunView[], query: string): RunQueryResult {
  const needle = query.toLocaleLowerCase('ru');
  const matched = runs.filter((r) =>
    [r.name, ...r.drops.map((d) => d.name), ...r.costs.map((c) => c.name)]
      .join(' | ')
      .toLocaleLowerCase('ru')
      .includes(needle),
  );
  const rates = matched.map((r) => r.perHour).sort((a, b) => a - b);
  const median = rates.length > 0 ? rates[Math.floor(rates.length / 2)]! : 0;
  return {
    total: matched.length,
    netFe: matched.reduce((a, r) => a + r.netFe, 0),
    inMapMs: matched.reduce((a, r) => a + r.activeMs, 0),
    rate: {
      median,
      mean: median,
      p25: median,
      p75: median,
      sampleSize: rates.length,
      confidence: 'insufficient',
      topShare: 0,
    },
    runs: matched,
  };
}

/**
 * Забеги: последние на экране, остальная история — поиском.
 *
 * Строка раскрывается в состав: что выпало и чем открывали. Раньше
 * добыча прошлого забега лежала в базе, но посмотреть её было негде.
 */
export function RecentRuns({ snapshot }: { snapshot: Snapshot }) {
  const [query, setQuery] = useState('');
  const [found, setFound] = useState<RunQueryResult | null>(null);
  const [openKey, setOpenKey] = useState<string | null>(null);
  const [exportNote, setExportNote] = useState<string | null>(null);

  // Новый забег в срезе — повод пересчитать найденное, иначе итоги
  // поиска застывали на моменте ввода.
  const latestKey = snapshot.recent[0]?.key ?? '';
  useEffect(() => {
    const q = query.trim();
    if (q === '') {
      setFound(null);
      return;
    }
    const timer = setTimeout(() => {
      if (window.gorn) void window.gorn.runs.query(q).then(setFound);
      else setFound(localQuery(snapshot.recent, q));
    }, 250);
    return () => clearTimeout(timer);
  }, [query, latestKey, snapshot.playerFilter]);

  const searching = query.trim() !== '';
  const runs = searching ? (found?.runs ?? []) : snapshot.recent;
  const median = snapshot.norm;
  if (snapshot.recent.length === 0 && !searching) return null;

  const exportCsv = async () => {
    if (!window.gorn) return;
    const res = await window.gorn.runs.exportCsv();
    if (res.status === 'saved') setExportNote(tr(`Сохранено: ${res.path}`, `Saved: ${res.path}`));
    else if (res.status === 'error') {
      setExportNote(tr(`Не удалось сохранить: ${res.message}`, `Couldn't save: ${res.message}`));
    }
    else setExportNote(null);
  };

  return (
    <section className="card">
      <div className="runs-head">
        <h2>{tr('Забеги', 'Runs')}</h2>
        <div className="runs-tools">
          <input
            type="search"
            className="runs-search"
            placeholder={tr('Поиск: карта, предмет, компас…', 'Search: map, item, compass…')}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            aria-label={tr('Поиск по всем забегам', 'Search all runs')}
          />
          <button
            className="session-reset"
            onClick={() => void exportCsv()}
            disabled={!window.gorn}
            title={tr(
              'Все забеги в CSV для Excel: время, карта, стратегия, добыча, вложения, чистыми. С учётом фильтра по персонажу.',
              'All runs to CSV for Excel: time, map, strategy, loot, map costs, net. Respects the character filter.',
            )}
          >
            ⤓ CSV
          </button>
        </div>
        {!searching && <RateHistogram snapshot={snapshot} />}
      </div>
      {exportNote && <p className="muted small runs-note">{exportNote}</p>}
      {searching && found && (
        <p className="runs-summary">
          {found.total === 0 ? (
            tr('Ничего не нашлось.', 'Nothing found.')
          ) : (
            <>
              {pluralize(found.total, RUN)} · {tr('чистыми', 'net')}{' '}
              <Fe value={found.netFe} signed /> ·{' '}
              {tr('медиана', 'median')} {num(found.rate.median)}/{tr('час', 'h')} ·{' '}
              {duration(found.inMapMs)} {tr('в картах', 'in maps')}
              {found.total > found.runs.length && (
                <span className="muted">
                  {' · '}
                  {tr(
                    `показаны последние ${found.runs.length}`,
                    `showing the latest ${found.runs.length}`,
                  )}
                </span>
              )}
            </>
          )}
        </p>
      )}
      {runs.length > 0 && (
        <table className="runs">
          <thead>
            <tr>
              <th>{tr('Время', 'Time')}</th>
              <th>{tr('Карта', 'Map')}</th>
              <th className="r">{tr('Длительность', 'Duration')}</th>
              <th className="r">
                {tr('Чистыми', 'Net')} <FeIcon />
              </th>
              <th className="r">{tr('В час', 'Per hour')}</th>
              <th className="r">
                <Tip
                  tip={tr(
                    'Отклонение темпа забега от «нормы» — медианы последних забегов.',
                    "How far the run's rate is from the “norm”: the median of recent runs.",
                  )}
                >
                  {tr('К норме', 'Vs norm')}
                </Tip>
              </th>
            </tr>
          </thead>
          <tbody>
            {runs.map((r) => {
              const delta = median > 0 ? Math.round(((r.perHour - median) / median) * 100) : 0;
              const isOpen = openKey === r.key;
              const toggle = () => setOpenKey(isOpen ? null : r.key);
              // Поиск достаёт и старые забеги — им нужна дата, не только время.
              const when =
                new Date(r.startedAt).toDateString() === new Date().toDateString()
                  ? time(r.startedAt)
                  : cacheDate(r.startedAt);
              return [
                <tr
                  key={r.key}
                  className={`run-row ${isOpen ? 'run-row-open' : ''}`}
                  tabIndex={0}
                  aria-expanded={isOpen}
                  onClick={toggle}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter' || e.key === ' ') {
                      e.preventDefault();
                      toggle();
                    }
                  }}
                >
                  <td className="muted">{when}</td>
                  <td>
                    <span className="run-chevron" aria-hidden>
                      {isOpen ? '▾' : '▸'}
                    </span>
                    {r.name}
                  </td>
                  <td className="r muted">{duration(r.durationMs ?? 0)}</td>
                  <td className="r">
                    <Fe value={r.netFe} />
                  </td>
                  <td className="r muted">{num(r.perHour)}</td>
                  <td className={`r ${delta >= 0 ? 'up' : 'down'}`}>
                    {median > 0 ? `${delta >= 0 ? '+' : '−'}${Math.abs(delta)}%` : '—'}
                  </td>
                </tr>,
                isOpen && (
                  <tr key={`${r.key}-detail`} className="run-detail">
                    <td colSpan={6}>
                      <div className="run-detail-grid">
                        <div>
                          <h3>
                            {tr('Выпало', 'Loot')} · <Fe value={r.valueFe} />
                          </h3>
                          <DropList drops={r.drops} minFe={snapshot.minDropFe} />
                        </div>
                        <div>
                          <h3>
                            {tr('Вложено', 'Invested')} · <Fe value={r.costFe} />
                          </h3>
                          <DropList
                            drops={r.costs}
                            empty={tr('Без вложений.', 'No map costs.')}
                          />
                        </div>
                      </div>
                    </td>
                  </tr>
                ),
              ];
            })}
          </tbody>
        </table>
      )}
    </section>
  );
}
