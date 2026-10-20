import { useState } from 'react';

import { tr } from '../../core/i18n.js';
import { RUN, pluralize } from '../../core/plural.js';
import type { Snapshot } from '../../main/engine.js';
import { dayFull, dayLabel, duration, num, shortDate } from '../format.js';
import { Fe, FeIcon } from './Fe.js';

type Day = Snapshot['days'][number];

/**
 * Темп периода. Тюрьма в итоге есть, а в темпе нет: её время лог не видит,
 * и её добыча, делённая на чужие минуты, задрала бы «в час».
 */
const rateOf = (t: { netFe: number; prisonFe: number; inMapMs: number }) =>
  t.inMapMs > 0 ? ((t.netFe - t.prisonFe) / t.inMapMs) * 3_600_000 : 0;

/**
 * Чистыми по дням за две недели. Одна серия — легенда не нужна,
 * заголовок называет её сам. Отрицательный день уходит под ноль:
 * вложения без отдачи тоже должны быть видны, а не обрезаны.
 */
function DaysChart({ days }: { days: Day[] }) {
  const [hover, setHover] = useState<number | null>(null);

  const w = 420;
  const h = 120;
  const labelH = 16;
  const gap = 4;
  const max = Math.max(...days.map((d) => d.netFe), 0);
  const min = Math.min(...days.map((d) => d.netFe), 0);
  const span = max - min || 1;
  const plotH = h - labelH;
  const zeroY = (max / span) * plotH;
  const barW = (w - gap * (days.length - 1)) / days.length;
  const today = days.length - 1;

  // Скругление 3px только у конца данных, основание у нуля — прямое.
  const bar = (x: number, value: number) => {
    const len = (Math.abs(value) / span) * plotH;
    if (len < 0.5) return null;
    const r = Math.min(3, len, barW / 2);
    if (value >= 0) {
      const y = zeroY - len;
      return `M${x},${zeroY} V${y + r} Q${x},${y} ${x + r},${y} H${x + barW - r} Q${x + barW},${y} ${x + barW},${y + r} V${zeroY} Z`;
    }
    const y = zeroY + len;
    return `M${x},${zeroY} V${y - r} Q${x},${y} ${x + r},${y} H${x + barW - r} Q${x + barW},${y} ${x + barW},${y - r} V${zeroY} Z`;
  };

  const active = hover === null ? null : days[hover];

  return (
    <div className="days">
      <svg
        viewBox={`0 0 ${w} ${h}`}
        className="days-svg"
        role="img"
        aria-label={tr('Чистыми по дням за две недели', 'Net by day, last two weeks')}
        onMouseLeave={() => setHover(null)}
      >
        <line x1={0} x2={w} y1={zeroY} y2={zeroY} className="days-zero" />
        {days.map((d, i) => {
          const x = i * (barW + gap);
          const path = bar(x, d.netFe);
          return (
            <g key={d.day}>
              {path && (
                <path
                  d={path}
                  className={`days-bar ${d.netFe < 0 ? 'days-bar-neg' : ''} ${hover === i ? 'days-bar-hover' : ''}`}
                />
              )}
              {/* Цель наведения — на всю высоту колонки, а не на столбик:
                  в пустой день навести было бы не на что. */}
              <rect
                x={x - gap / 2}
                y={0}
                width={barW + gap}
                height={h}
                className="days-hit"
                tabIndex={0}
                aria-label={tr(
                  `${dayFull(d.day)}: ${num(d.netFe)} ФЕ, ${pluralize(d.runs, RUN)}`,
                  `${dayFull(d.day)}: ${num(d.netFe)} FE, ${pluralize(d.runs, RUN)}`,
                )}
                onMouseEnter={() => setHover(i)}
                onFocus={() => setHover(i)}
                onBlur={() => setHover(null)}
              />
              {(i % 2 === today % 2 || i === today) && (
                <text
                  x={x + barW / 2}
                  y={h - 3}
                  textAnchor="middle"
                  className={`days-label ${i === today ? 'days-label-today' : ''}`}
                >
                  {i === today ? tr('сегодня', 'today') : dayLabel(d.day)}
                </text>
              )}
            </g>
          );
        })}
      </svg>
      <div className="days-tip" aria-live="polite">
        {active ? (
          active.runs + active.prisonRuns === 0 ? (
            <>
              <b>{dayFull(active.day)}</b> · {tr('не играл', "didn't play")}
            </>
          ) : (
            <>
              <b>{dayFull(active.day)}</b> · <Fe value={active.netFe} signed /> ·{' '}
              {pluralize(active.runs, RUN)} · {duration(active.inMapMs)}{' '}
              {tr('в картах', 'in maps')} ·{' '}
              {num(rateOf(active))}/{tr('час', 'h')}
            </>
          )
        ) : (
          <span className="muted">
            {tr('Наведи на день, чтобы увидеть подробности.', 'Hover over a day to see details.')}
          </span>
        )}
      </div>
    </div>
  );
}

/**
 * История: дни и сессии. Раньше сброс сессии оставлял на экране только
 * итог одной прошлой — дальше история была видна лишь в базе.
 */
export function History({ snapshot }: { snapshot: Snapshot }) {
  const { days, sessions } = snapshot;
  const played = days.filter((d) => d.runs + d.prisonRuns > 0);
  if (played.length === 0 && sessions.length === 0) return null;

  const total = played.reduce((a, d) => a + d.netFe, 0);
  const totalMs = played.reduce((a, d) => a + d.inMapMs, 0);
  const totalPrison = played.reduce((a, d) => a + d.prisonFe, 0);

  return (
    <section className="card">
      <h2>{tr('История', 'History')}</h2>
      <div className="history-grid">
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
          <div>
            <p className="history-sum">
              {tr('За две недели:', 'Last two weeks:')} <Fe value={total} signed /> ·{' '}
              {duration(totalMs)}{' '}
              {tr('в картах', 'in maps')} · {num(rateOf({ netFe: total, prisonFe: totalPrison, inMapMs: totalMs }))}/{tr('час', 'h')}
              <span className="muted">
                {' · '}
                {tr(`дней с игрой: ${played.length}`, `days played: ${played.length}`)}
              </span>
            </p>
            <DaysChart days={days} />
          </div>
        )}
        {sessions.length > 0 && (
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
              {sessions.map((s) => (
                <tr key={s.startedAt} className={s.endedAt === null ? 'session-current' : ''}>
                  <td>
                    {s.startedAt === 0
                      ? tr('до первого сброса', 'before the first reset')
                      : tr(`с ${shortDate(s.startedAt)}`, `since ${shortDate(s.startedAt)}`)}
                    {s.endedAt === null && (
                      <span className="muted"> · {tr('текущая', 'current')}</span>
                    )}
                  </td>
                  <td className="r muted">{s.runs}</td>
                  <td className="r muted">{duration(s.inMapMs)}</td>
                  <td className="r">
                    <Fe value={s.netFe} />
                  </td>
                  <td className="r muted">{num(rateOf(s))}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </section>
  );
}
