import { useState } from 'react';

import { tr } from '../../core/i18n.js';
import { RUN, pluralize } from '../../core/plural.js';
import type { Snapshot } from '../../main/engine.js';
import { dayFull, dayLabel, duration, num } from '../format.js';
import { Fe } from './Fe.js';

type Day = Snapshot['days'][number];

/**
 * Темп периода. Тюрьма в итоге есть, а в темпе нет: её время лог не видит,
 * и её добыча, делённая на чужие минуты, задрала бы «в час».
 */
export const rateOf = (t: { netFe: number; prisonFe: number; inMapMs: number }) =>
  t.inMapMs > 0 ? ((t.netFe - t.prisonFe) / t.inMapMs) * 3_600_000 : 0;

/**
 * Чистыми по дням за две недели. Одна серия — легенда не нужна,
 * заголовок называет её сам. Отрицательный день уходит под ноль:
 * вложения без отдачи тоже должны быть видны, а не обрезаны.
 */
export function DaysChart({ days }: { days: Day[] }) {
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
