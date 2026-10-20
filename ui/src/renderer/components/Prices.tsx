import { useEffect, useState } from 'react';

import type { PriceRow, Snapshot } from '../../main/engine.js';
import { tr } from '../../core/i18n.js';
import { ago, num } from '../format.js';
import { Fe } from './Fe.js';
import { CopyName, ItemIcon, Tip } from './ui.js';

/** Сколько строк видно сразу: остальное — по кнопке. */
const VISIBLE = 12;

/**
 * Спарклайн своих пробивок за месяц. Одна серия — легенда не нужна;
 * точка — свежая пробивка, к ней и относится цена в строке.
 */
function PriceSpark({ row }: { row: PriceRow }) {
  const pts = row.trend.points;
  if (pts.length < 2) {
    return (
      <span className="price-spark-empty muted small">
        {pts.length === 0
          ? tr('своих пробивок нет', 'no price checks')
          : tr('одна пробивка', 'one price check')}
      </span>
    );
  }
  const w = 72;
  const h = 20;
  const t0 = pts[0]!.t;
  const tSpan = pts.at(-1)!.t - t0 || 1;
  const lo = Math.min(...pts.map((p) => p.fe));
  const hi = Math.max(...pts.map((p) => p.fe));
  const span = hi - lo || 1;
  const x = (t: number) => 2 + ((t - t0) / tSpan) * (w - 4);
  const y = (fe: number) => h - 3 - ((fe - lo) / span) * (h - 6);
  const d = pts.map((p, i) => `${i === 0 ? 'M' : 'L'}${x(p.t)},${y(p.fe)}`).join(' ');
  const last = pts.at(-1)!;
  return (
    <svg
      viewBox={`0 0 ${w} ${h}`}
      width={w}
      height={h}
      className="price-spark"
      role="img"
      aria-label={tr(
        `${pts.length} пробивок за месяц: от ${num(lo, 2)} до ${num(hi, 2)} ФЕ`,
        `${pts.length} price checks this month: ${num(lo, 2)} to ${num(hi, 2)} FE`,
      )}
    >
      <title>
        {tr(
          `${pts.length} пробивок за месяц: ${num(lo, 2)}–${num(hi, 2)} ФЕ`,
          `${pts.length} price checks this month: ${num(lo, 2)}–${num(hi, 2)} FE`,
        )}
      </title>
      <path d={d} className="price-spark-line" />
      <circle cx={x(last.t)} cy={y(last.fe)} r={2.5} className="price-spark-dot" />
    </svg>
  );
}

function Change({ change }: { change: number | null }) {
  if (change === null) return <span className="muted">—</span>;
  const pct = Math.round(change * 100);
  if (pct === 0) return <span className="muted">0%</span>;
  return (
    <span className={pct > 0 ? 'up' : 'down'}>
      {pct > 0 ? '↑' : '↓'} {Math.abs(pct)}%
    </span>
  );
}

function Source({ row }: { row: PriceRow }) {
  if (row.source === 'excluded') {
    return <span className="muted">{tr('не учитывается', 'ignored')}</span>;
  }
  if (row.source === 'community') {
    return (
      <Tip
        tip={tr(
          'Цена из базы сообщества (titrack.ninja): своей пробивки нет или она старше трёх дней и сообщество знает новее.',
          'Community price (titrack.ninja): you have no price check of your own, or yours is over three days old and the community has a newer one.',
        )}
      >
        <span className="price-community">{tr('сообщество', 'community')}</span>
      </Tip>
    );
  }
  if (row.ownObservedAt !== null) {
    return (
      <span className="muted">
        {tr('своя', 'own')} · {ago(Date.now() - row.ownObservedAt)}
      </span>
    );
  }
  return <span className="muted">—</span>;
}

/**
 * Цены: по чему считается добыча, откуда цена и куда она движется.
 *
 * По умолчанию — то, что падает тебе, по влиянию на оценку; поиск —
 * по всему справочнику. Здесь же исключение из учёта и его отмена:
 * исключённое стоит в начале списка, иначе его было бы не вернуть.
 */
export function Prices({ snapshot }: { snapshot: Snapshot }) {
  const [query, setQuery] = useState('');
  const [rows, setRows] = useState<PriceRow[] | null>(null);
  const [all, setAll] = useState(false);
  const [version, setVersion] = useState(0);

  // Новые пробивки и цены сообщества — повод перечитать список.
  const priceKey = `${snapshot.diagnostics.pricedItems}:${snapshot.diagnostics.communityPrices}:${snapshot.recent[0]?.key ?? ''}`;
  useEffect(() => {
    const timer = setTimeout(
      () => {
        if (window.gorn) void window.gorn.prices.query(query.trim()).then(setRows);
        else if (import.meta.env.DEV) {
          void import('../demo.js').then((m) =>
            setRows(
              m.demoPrices.filter((r) =>
                r.name.toLocaleLowerCase('ru').includes(query.trim().toLocaleLowerCase('ru')),
              ),
            ),
          );
        }
      },
      query === '' ? 0 : 250,
    );
    return () => clearTimeout(timer);
  }, [query, priceKey, version]);

  if (rows === null) return null;
  if (rows.length === 0 && query === '') return null;

  const toggle = async (row: PriceRow) => {
    await window.gorn?.prices.setExcluded(row.configBaseId, row.source !== 'excluded');
    setVersion((v) => v + 1);
  };
  const shown = all ? rows : rows.slice(0, VISIBLE);

  return (
    <section className="card">
      <div className="runs-head">
        <h2>{tr('Цены', 'Prices')}</h2>
        <div className="runs-tools">
          <input
            type="search"
            className="runs-search"
            placeholder={tr('Найти предмет…', 'Find an item…')}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            aria-label={tr('Поиск цены по справочнику', 'Search prices across all items')}
          />
        </div>
      </div>
      {rows.length === 0 ? (
        <p className="muted">{tr('Ничего не нашлось.', 'Nothing found.')}</p>
      ) : (
        <table className="runs prices">
          <thead>
            <tr>
              <th>{tr('Предмет', 'Item')}</th>
              <th className="r">{tr('Цена', 'Price')}</th>
              <th>
                <Tip
                  tip={tr(
                    'Свои пробивки за месяц и изменение к самой ранней пробивке в пределах недели.',
                    'Your own price checks this month and the change since your earliest check within the past week.',
                  )}
                >
                  {tr('Тренд', 'Trend')}
                </Tip>
              </th>
              <th>{tr('Источник', 'Source')}</th>
              <th className="r">{tr('Встречалось', 'Seen')}</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {shown.map((row) => (
              <tr key={row.configBaseId} className={row.source === 'excluded' ? 'map-weak' : ''}>
                <td>
                  <div className="price-item">
                    <ItemIcon src={row.icon} />
                    <CopyName name={row.name} />
                  </div>
                </td>
                <td className="r">
                  {row.source === 'excluded' ? (
                    <span className="muted">—</span>
                  ) : row.fe === null ? (
                    <span className="muted">{tr('нет цены', 'no price')}</span>
                  ) : (
                    <Fe value={row.fe} />
                  )}
                </td>
                <td>
                  <div className="price-trend">
                    <PriceSpark row={row} />
                    {row.trend.points.length > 1 && <Change change={row.trend.change} />}
                  </div>
                </td>
                <td>
                  <Source row={row} />
                </td>
                <td className="r muted">{row.seenQty > 0 ? `×${num(row.seenQty)}` : '—'}</td>
                <td className="r">
                  <button
                    className="link-button"
                    disabled={!window.gorn}
                    title={
                      row.source === 'excluded'
                        ? tr(
                            'Снова считать предмет по его цене.',
                            'Count the item at its price again.',
                          )
                        : tr(
                            'Считать предмет бесплатным во всех цифрах: для того, что не продаётся.',
                            "Count the item as free in all numbers: for things that don't sell.",
                          )
                    }
                    onClick={() => void toggle(row)}
                  >
                    {row.source === 'excluded'
                      ? tr('вернуть', 'restore')
                      : tr('не учитывать', 'ignore')}
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {rows.length > VISIBLE && (
        <button className="link-button prices-more" onClick={() => setAll(!all)}>
          {all
            ? tr('свернуть', 'collapse')
            : tr(`показать все ${rows.length}`, `show all ${rows.length}`)}
        </button>
      )}
    </section>
  );
}
