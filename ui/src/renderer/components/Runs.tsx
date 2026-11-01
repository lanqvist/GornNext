import { useState } from 'react';

import { tr } from '../../core/i18n.js';
import { RUN, pluralize } from '../../core/plural.js';
import type { RunQueryResult, RunView } from '../../main/engine.js';
import { cacheDate, duration, num, time } from '../format.js';
import { DropList } from './DropList.js';
import { Fe, FeIcon } from './Fe.js';
import { Tip } from './ui.js';

/**
 * Поиск в браузерном демо: main-процесса нет, ищем по забегам из среза.
 * В приложении поиск идёт по всей истории в main.
 */
export function localQuery(runs: RunView[], query: string): RunQueryResult {
  const needle = query.toLocaleLowerCase('ru');
  const matched = runs.filter((r) =>
    [r.name, ...r.drops.map((d) => d.name), ...r.costs.map((c) => c.name)]
      .join(' | ')
      .toLocaleLowerCase('ru')
      .includes(needle),
  );
  const rates = matched.map((r) => r.perHour).sort((a, b) => a - b);
  const median = rates.length > 0 ? rates[Math.floor(rates.length / 2)]! : 0;
  const netFe = matched.reduce((a, r) => a + r.netFe, 0);
  const inMapMs = matched.reduce((a, r) => a + r.activeMs, 0);
  return {
    total: matched.length,
    netFe,
    inMapMs,
    rate: {
      perHour: inMapMs > 0 ? (netFe / inMapMs) * 3_600_000 : 0,
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
 * Таблица забегов: строка раскрывается в состав — что выпало и чем
 * открывали. Живёт в «Журнале»: под сессией и в результатах поиска.
 */
export function RunsTable({
  runs,
  norm,
  minDropFe,
}: {
  runs: RunView[];
  norm: number;
  minDropFe: number;
}) {
  const [openKey, setOpenKey] = useState<string | null>(null);
  if (runs.length === 0) return null;
  return (
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
                'Темп забега к обычному — сколько в час дали последние 20 забегов вместе.',
                "The run's pace vs usual: the per-hour pace of the last 20 runs together.",
              )}
            >
              {tr('К обычному', 'Vs usual')}
            </Tip>
          </th>
        </tr>
      </thead>
      <tbody>
        {runs.map((r) => {
          const delta = norm > 0 ? Math.round(((r.perHour - norm) / norm) * 100) : 0;
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
                {norm > 0 ? `${delta >= 0 ? '+' : '−'}${Math.abs(delta)}%` : '—'}
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
                      <DropList drops={r.drops} minFe={minDropFe} />
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
    </table>  );
}

/** Итог найденного поиском: сколько, чистыми, в час, сколько времени. */
export function RunsSummary({ found }: { found: RunQueryResult }) {
  return (
    <p className="runs-summary">
      {found.total === 0 ? (
        tr('Ничего не нашлось.', 'Nothing found.')
      ) : (
        <>
          {pluralize(found.total, RUN)} · {tr('чистыми', 'net')} <Fe value={found.netFe} signed /> ·{' '}
          {num(found.rate.perHour)} {tr('в час', 'per hour')} · {duration(found.inMapMs)}{' '}
          {tr('в картах', 'in maps')}
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
  );
}
