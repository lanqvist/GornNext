import { Fragment, useEffect, useState } from 'react';

import { tr } from '../../core/i18n.js';
import { VISIT, pluralize } from '../../core/plural.js';
import type { RunQueryResult, RunView, Snapshot } from '../../main/engine.js';
import { duration, num, shortDate } from '../format.js';
import { DaysChart, rateOf } from './Days.js';
import { Fe, FeIcon } from './Fe.js';
import { RunsSummary, RunsTable, localQuery } from './Runs.js';

type Session = Snapshot['sessions'][number];

const sessionKey = (s: Session) => `${s.startedAt}`;

/**
 * Забеги сессии в браузерном демо: main-процесса нет, берём из среза.
 * Забег принадлежит сессии, в которой закончился, — как в main.
 */
function localSession(runs: RunView[], s: Session): RunQueryResult {
  const inside = runs.filter((r) => {
    const end = r.startedAt + (r.durationMs ?? 0);
    return end >= s.startedAt && (s.endedAt === null || end < s.endedAt);
  });
  const result = localQuery(inside, '');
  return { ...result, runs: inside };
}

/** Забеги одной сессии: подгружаются при раскрытии, текущая — с каждым новым забегом. */
function SessionRuns({ session, snapshot }: { session: Session; snapshot: Snapshot }) {
  const [found, setFound] = useState<RunQueryResult | null>(null);
  const live = session.endedAt === null ? (snapshot.recent[0]?.key ?? '') : '';

  useEffect(() => {
    if (window.gorn) {
      void window.gorn.runs.session(session.startedAt, session.endedAt).then(setFound);
    } else {
      setFound(localSession(snapshot.recent, session));
    }
  }, [session.startedAt, session.endedAt, live]);

  if (!found) return <p className="muted small journal-loading">{tr('Загружаю…', 'Loading…')}</p>;
  if (found.runs.length === 0) {
    return (
      <p className="muted small journal-loading">
        {tr('В этой сессии — только тюрьма.', 'Only prison in this session.')}
      </p>
    );
  }
  return (
    <>
      {found.total > found.runs.length && (
        <p className="muted small">
          {tr(`показаны последние ${found.runs.length}`, `showing the latest ${found.runs.length}`)}
        </p>
      )}
      <RunsTable runs={found.runs} norm={snapshot.norm} minDropFe={snapshot.minDropFe} />
    </>
  );
}

/**
 * Журнал: что было. Сверху — чистыми по дням за две недели, под ними —
 * сессии; сессия раскрывается в свои забеги, забег — в состав. Поиск
 * идёт по всей истории сразу. Раньше это были две вкладки: «Забеги»
 * повторяли график темпа сводки, «История» не давала заглянуть внутрь.
 */
export function Journal({ snapshot }: { snapshot: Snapshot }) {
  const { days, sessions } = snapshot;
  const [query, setQuery] = useState('');
  const [found, setFound] = useState<RunQueryResult | null>(null);
  const [exportNote, setExportNote] = useState<string | null>(null);
  // Текущая сессия раскрыта сразу: её забеги — то, за чем сюда приходят чаще всего.
  const [open, setOpen] = useState<Set<string>>(() => {
    const current = sessions.find((s) => s.endedAt === null);
    return new Set(current ? [sessionKey(current)] : []);
  });

  // Новый забег — повод пересчитать найденное, иначе итоги поиска
  // застывали на моменте ввода.
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
  }, [query, latestKey]);

  const played = days.filter((d) => d.runs + d.prisonRuns > 0);
  const total = played.reduce((a, d) => a + d.netFe, 0);
  const totalMs = played.reduce((a, d) => a + d.inMapMs, 0);
  const totalPrison = played.reduce((a, d) => a + d.prisonFe, 0);
  const searching = query.trim() !== '';

  const exportCsv = async () => {
    if (!window.gorn) return;
    const res = await window.gorn.runs.exportCsv();
    if (res.status === 'saved') setExportNote(tr(`Сохранено: ${res.path}`, `Saved: ${res.path}`));
    else if (res.status === 'error') {
      setExportNote(tr(`Не удалось сохранить: ${res.message}`, `Couldn't save: ${res.message}`));
    } else setExportNote(null);
  };

  const toggle = (key: string) =>
    setOpen((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });

  return (
    <section className="card journal">
      {/* Ряд пустых столбиков и «+0 ФЕ» после перерыва выглядели бы
          поломкой, а не паузой. */}
      {played.length === 0 ? (
        <p className="muted history-sum">
          {tr(
            'За последние две недели забегов нет — график появится с первым.',
            'No runs in the last two weeks: the chart will appear with your first one.',
          )}
        </p>
      ) : (
        <div className="journal-days">
          <p className="history-sum">
            {tr('За две недели', 'Last two weeks')} <Fe value={total} signed /> ·{' '}
            {duration(totalMs)} {tr('в картах', 'in maps')} ·{' '}
            {num(rateOf({ netFe: total, prisonFe: totalPrison, inMapMs: totalMs }))}/
            {tr('час', 'h')}
            <span className="muted">
              {' · '}
              {tr(`дней с игрой: ${played.length}`, `days played: ${played.length}`)}
            </span>
          </p>
          <DaysChart days={days} />
        </div>
      )}

      <div className="runs-head journal-head">
        <h2>{searching ? tr('Найденные забеги', 'Found runs') : tr('Сессии', 'Sessions')}</h2>
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
              'Все забеги в CSV для Excel: время, карта, стратегия, добыча, вложения, чистыми.',
              'All runs to CSV for Excel: time, map, strategy, loot, map costs, net.',
            )}
          >
            ⤓ CSV
          </button>
        </div>
      </div>
      {exportNote && <p className="muted small runs-note">{exportNote}</p>}

      {searching ? (
        found && (
          <>
            <RunsSummary found={found} />
            <RunsTable runs={found.runs} norm={snapshot.norm} minDropFe={snapshot.minDropFe} />
          </>
        )
      ) : sessions.length === 0 ? (
        <RunsTable runs={snapshot.recent} norm={snapshot.norm} minDropFe={snapshot.minDropFe} />
      ) : (
        <table className="runs sessions">
          <thead>
            <tr>
              <th>{tr('Сессия', 'Session')}</th>
              <th className="r">{tr('Забегов', 'Runs')}</th>
              <th className="r">{tr('В картах', 'In maps')}</th>
              <th className="r">
                {tr('Чистыми', 'Net')} <FeIcon />
              </th>
              <th className="r">{tr('В час', 'Per hour')}</th>
            </tr>
          </thead>
          <tbody>
            {sessions.map((s) => {
              const key = sessionKey(s);
              const isOpen = open.has(key);
              return (
                <Fragment key={key}>
                  <tr
                    className={`run-row session-row ${isOpen ? 'run-row-open' : ''} ${s.endedAt === null ? 'session-current' : ''}`}
                    tabIndex={0}
                    aria-expanded={isOpen}
                    onClick={() => toggle(key)}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter' || e.key === ' ') {
                        e.preventDefault();
                        toggle(key);
                      }
                    }}
                  >
                    <td>
                      <span className="run-chevron" aria-hidden>
                        {isOpen ? '▾' : '▸'}
                      </span>
                      {s.startedAt === 0
                        ? tr('до первого сброса', 'before the first reset')
                        : tr(`с ${shortDate(s.startedAt)}`, `since ${shortDate(s.startedAt)}`)}
                      {s.endedAt === null && (
                        <span className="muted"> · {tr('текущая', 'current')}</span>
                      )}
                    </td>
                    <td className="r muted">
                      {s.runs}
                      {s.prisonRuns > 0 && (
                        <span
                          className="session-prison"
                          title={tr(
                            `и ${pluralize(s.prisonRuns, VISIT)} в тюрьму — они в списке забегов, но не в темпе`,
                            `plus ${pluralize(s.prisonRuns, VISIT)} to prison — listed with the runs, not in the pace`,
                          )}
                        >
                          {' '}+{s.prisonRuns}
                        </span>
                      )}
                    </td>
                    <td className="r muted">{duration(s.inMapMs)}</td>
                    <td className="r">
                      <Fe value={s.netFe} />
                    </td>
                    <td className="r muted">{num(rateOf(s))}</td>
                  </tr>
                  {isOpen && (
                    <tr className="session-detail">
                      <td colSpan={5}>
                        <SessionRuns session={s} snapshot={snapshot} />
                      </td>
                    </tr>
                  )}
                </Fragment>
              );
            })}
          </tbody>
        </table>
      )}
    </section>
  );
}
