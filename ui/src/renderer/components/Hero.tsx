import { useRef } from 'react';

import { tr } from '../../core/i18n.js';
import { RUN, VISIT, pluralize } from '../../core/plural.js';
import { NORM_WINDOW } from '../../core/analysis.js';
import type { Snapshot } from '../../main/engine.js';
import { NBSP, cacheDate, duration, num, time } from '../format.js';
import { useTween, useWidth } from '../motion.js';
import { Fe } from './Fe.js';
import { SessionRecords } from './SessionRecords.js';
import { Tip } from './ui.js';

/** Меньше этого времени на картах темп сессии — шум одной находки, жар не показываем. */
const SESSION_WARMUP_MS = 5 * 60_000;

/**
 * Темп карт за сессию к обычному, %. По нему калится окно — кромка сверху
 * и жар под главной цифрой, — тем же цветом, что кромка оверлея на карте.
 * Сравнение — карты с картами: обычное (в час за последние 20 забегов)
 * тюрьму не знает, и с ней сессия выглядела вдвое жарче, чем шли карты.
 * null — сравнивать пока не с чем.
 */
export function sessionDelta(snapshot: Snapshot): number | null {
  const { totals, prison, norm } = snapshot;
  if (norm <= 0 || totals.inMapMs < SESSION_WARMUP_MS) return null;
  const mapsRate = ((totals.valueFe - totals.costFe - prison.netFe) / totals.inMapMs) * 3_600_000;
  return Math.round(((mapsRate - norm) / norm) * 100);
}

/**
 * Спарклайн темпа во всю ширину: последние забеги в хронологии. Пунктир —
 * в час за последние 20 забегов (норма), точка — самый свежий забег.
 * Рисуется под измеренную ширину: растянутый viewBox искажал бы точки.
 */
function RateSparkline({ snapshot }: { snapshot: Snapshot }) {
  const box = useRef<HTMLDivElement>(null);
  const w = useWidth(box, 600);
  const runs = [...snapshot.recent].reverse();
  if (runs.length < 5) return null;

  const values = runs.map((r) => r.perHour);
  const norm = snapshot.norm;
  const h = 64;
  const pad = 6;
  const max = Math.max(...values, norm);
  const min = Math.min(...values, norm, 0);
  const span = max - min || 1;
  const x = (i: number) => pad + (i / (values.length - 1)) * (w - pad * 2);
  const y = (v: number) => h - pad - ((v - min) / span) * (h - pad * 2);
  const points = values.map((v, i) => `${x(i).toFixed(1)},${y(v).toFixed(1)}`);
  const last = values.length - 1;
  const area = `M${points.join(' L')} L${x(last)},${h} L${x(0)},${h} Z`;

  return (
    <div className="spark" ref={box}>
      <svg
        viewBox={`0 0 ${w} ${h}`}
        width={w}
        height={h}
        role="img"
        aria-label={tr(
          `Темп последних ${values.length} забегов, ФЕ в час`,
          `Pace of the last ${values.length} runs, FE per hour`,
        )}
      >
        <defs>
          <linearGradient id="spark-fill" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0" className="spark-fill-top" />
            <stop offset="1" className="spark-fill-bottom" />
          </linearGradient>
        </defs>
        <path d={area} fill="url(#spark-fill)" />
        <line x1={pad} x2={w - pad} y1={y(norm)} y2={y(norm)} className="spark-median" />
        <polyline points={points.join(' ')} className="spark-line" />
        <circle cx={x(last)} cy={y(values[last]!)} r={9} className="spark-halo" />
        <circle cx={x(last)} cy={y(values[last]!)} r={3.5} className="spark-dot" />
        {/* Невидимые увеличенные цели наведения: подсказка на каждую точку. */}
        {values.map((v, i) => (
          <circle key={i} cx={x(i)} cy={y(v)} r={8} fill="transparent">
            <title>
              {`${time(runs[i]!.startedAt)} · ${runs[i]!.name}: ${num(v)} ${tr('ФЕ/час', 'FE/h')}`}
            </title>
          </circle>
        ))}
      </svg>
    </div>
  );
}

/** Главная цифра: доезжает до нового значения сама, остальной блок не трогает. */
function HeroNumber({ value }: { value: number }) {
  return <Fe value={useTween(value)} />;
}

function Stat({
  label,
  tip,
  value,
  warn,
}: {
  label: string;
  tip: string;
  value: React.ReactNode;
  /** Пояснение, почему цифре нельзя верить как есть. */
  warn?: string;
}) {
  return (
    <div className={`stat ${warn ? 'stat-warn' : ''}`}>
      <dt>
        <Tip tip={tip}>{label}</Tip>
      </dt>
      <dd>{value}</dd>
      {warn && <dd className="stat-note">{warn}</dd>}
    </div>
  );
}

/**
 * Главный блок без рамки: крупно — сколько выходит в час за сессию, вся
 * добыча за вычетом вложений ÷ время в зонах (карты, бои Vorax, тюрьма;
 * город не в счёт). Под ней — сама формула, чтобы цифру можно было
 * проверить в уме. Справа — четыре числа поменьше, ниже — темп забегов
 * во всю ширину и рекорды. Медианы здесь нет: она описывает «типичную
 * карту», а не сколько на деле приносит час фарма.
 */
export function Hero({ snapshot }: { snapshot: Snapshot }) {
  const { rate, feRate, totals } = snapshot;
  // «Вложено 0» при потраченном маяке выглядит как поломка. Причина обычно
  // одна: у расходника нет цены, и вычесть нечего.
  const unpricedCost = snapshot.missing.find((m) => m.isCost);
  const recentCount = Math.min(NORM_WINDOW, rate.sampleSize);
  const sessionNet = totals.valueFe - totals.costFe;
  const zoneMs = totals.inMapMs + totals.prisonMs;
  const delta = sessionDelta(snapshot);

  return (
    <section className="hero">
      <div className="hero-glow" aria-hidden="true" />
      <div className="hero-top">
        <div className="hero-main">
          <div className="hero-label">
            <Tip
              tip={tr(
                'Как у других трекеров — по зонам: вся добыча сессии по ценам минус вложения (карты, бои Vorax и тюрьма), делённая на активное время в них. Город не в счёт, пауза на карте дольше 3 минут — тоже; тюрьма и Vorax — целиком, от входа в их окно до выхода из зоны. Сессия начинается с запуском игры или кнопкой «Новая сессия».',
                'Like other trackers — by zone: all loot this session at prices minus map costs (maps, Vorax fights and prison), divided by active time in them. Town doesn’t count, nor do pauses longer than 3 minutes in a map; prison and Vorax count whole, from opening their window to leaving the zone. A session starts when the game launches or with the “New session” button.',
              )}
            >
              {tr('в час за сессию, за вычетом вложений', 'per hour this session, net of map costs')}
            </Tip>
          </div>
          <div className="hero-value">
            <span className="hero-num">
              <HeroNumber value={snapshot.liveRate} />
            </span>
            {delta !== null && (
              <Tip
                tip={tr(
                  `Карты этой сессии без тюрьмы — их добыча за вычетом вложений ÷ время на картах — к обычному: ${num(snapshot.norm)} ФЕ в час за последние ${pluralize(recentCount, RUN)}. Этим же цветом каления калится окно и кромка оверлея на карте.`,
                  `This session’s maps without prison — their loot net of costs ÷ time in maps — vs usual: ${num(snapshot.norm)} FE per hour over the last ${pluralize(recentCount, RUN)}. The window glows with the same heat colour as the overlay edge on a map.`,
                )}
              >
                <span className={`pill ${delta >= 0 ? 'up' : 'down'}`}>
                  {tr('карты', 'maps')} {delta >= 0 ? '↑' : '↓'} {Math.abs(delta)}%{' '}
                  {tr('к обычному', 'vs usual')}
                </span>
              </Tip>
            )}
          </div>
          <div className="hero-formula">
            = <Fe value={sessionNet} signed /> {tr('за', 'in')} {duration(zoneMs)}
            {snapshot.prison.runs > 0 && (
              <Tip
                tip={tr(
                  'Тюрьма Талоса входит в итог сессии и в «в час» вместе со своим временем, но не в темп карт, рекорды, карты и стратегии.',
                  'Tartarus Prison counts toward the session total and per hour together with its time, but not map pace, records, maps or strategies.',
                )}
              >
                <span className="hero-prison">
                  · {tr('тюрьма', 'prison')} <Fe value={snapshot.prison.netFe} signed />{' '}
                  {tr('за', 'in')} {pluralize(snapshot.prison.runs, VISIT)}
                </span>
              </Tip>
            )}
          </div>
        </div>

        <div className="hero-side">
          <dl className="hero-stats">
            <Stat
              label={tr('Итог сессии', 'Session total')}
              tip={tr(
                'Вся добыча сессии по ценам минус вложения в карты, вместе с тюрьмой.',
                'All loot this session at prices minus map costs, prison included.',
              )}
              value={<Fe value={sessionNet} signed />}
            />
            <Stat
              label={tr('Чистый ФЕ в час', 'Raw FE per hour')}
              tip={tr(
                'Только сама валюта ФЕ, упавшая в сумку, — без оценки предметов по ценам. За сезон: вся ФЕ ÷ всё время в картах.',
                'Only the FE currency itself that landed in your bag — no item valuation. Season-wide: all FE ÷ all time in maps.',
              )}
              value={<Fe value={feRate.perHour} />}
            />
            <Stat
              label={tr('На картах', 'In maps')}
              tip={tr(
                `Активное время на картах за сессию: пауза дольше 3 минут не в счёт. Всего сессия идёт ${duration(totals.sessionMs)} — с городом и тюрьмой.`,
                `Active time in maps this session: pauses longer than 3 minutes don't count. The whole session has run ${duration(totals.sessionMs)}, town and prison included.`,
              )}
              value={duration(totals.inMapMs)}
            />
            <Stat
              label={tr('Вложено', 'Invested')}
              tip={tr(
                'Стоимость всего, чем открывались карты за сессию: компасы, маяки и прочие расходники.',
                'The cost of everything used to open maps this session: compasses, beacons and other consumables.',
              )}
              value={<Fe value={totals.costFe} />}
              {...(unpricedCost
                ? { warn: tr(`${unpricedCost.name} без цены`, `${unpricedCost.name} has no price`) }
                : {})}
            />
          </dl>
          <SessionBar snapshot={snapshot} />
        </div>
      </div>

      <RateSparkline snapshot={snapshot} />
      <div className="hero-foot">
        <span className="hero-pace">
          <Tip
            tip={tr(
              'Только время на картах, без города: последние забеги вместе — их добыча за вычетом вложений ÷ их время. С этим темпом сравниваются карты.',
              'Map time only, no town: the latest runs together — their loot net of costs ÷ their time. Maps are compared against this pace.',
            )}
          >
            <span>
              {tr('на картах: последние', 'in maps: last')} {pluralize(recentCount, RUN)}{' '}
              <b>
                {num(snapshot.norm)}
                {tr(`${NBSP}в час`, '/h')}
              </b>
            </span>
          </Tip>
          <Tip
            tip={tr(
              'Все забеги сезона вместе: добыча за вычетом вложений ÷ время в картах, без города.',
              'All runs of the season together: loot net of costs ÷ time in maps, no town.',
            )}
          >
            <span>
              {tr('сезон', 'season')} {num(rate.perHour)}
              {tr(`${NBSP}в час`, '/h')} · {pluralize(rate.sampleSize, RUN)}
            </span>
          </Tip>
        </span>
        <SessionRecords snapshot={snapshot} />
      </div>
    </section>
  );
}

/**
 * Границы сессии: с какого момента считают цифры, и кнопка начать заново.
 *
 * Сброс ничего не удаляет — прошлые забеги остаются в истории и сезоне,
 * поэтому подтверждение не нужно. Итог прошлой сессии остаётся на экране:
 * без него сброс выглядит потерей цифр.
 */
function SessionBar({ snapshot }: { snapshot: Snapshot }) {
  const { startedAt, previous } = snapshot.session;
  // Вчерашняя сессия подписывается датой: «с 14:32» читается как сегодня.
  const started =
    new Date(startedAt).toDateString() === new Date().toDateString()
      ? time(startedAt)
      : cacheDate(startedAt);

  return (
    <div className="session-bar">
      <span>
        {startedAt > 0
          ? tr(`сессия с ${started}`, `session since ${started}`)
          : tr('сессия — вся история', 'session — all history')}
        {previous && (
          <>
            {' '}
            · {tr('прошлая', 'previous')} {previous.netFe >= 0 ? '+' : ''}
            {num(previous.netFe)}
            {NBSP}
            {tr('ФЕ за', 'FE in')} {duration(previous.inMapMs)}
          </>
        )}
      </span>
      <button
        className="ghost-btn"
        title={tr(
          'Темп сессии, цифры и цель оверлея начнут отсчёт заново. История, сезон и стратегии никуда не денутся.',
          'The session pace, numbers and the overlay goal will start counting from scratch. History, season and strategies stay put.',
        )}
        onClick={() => window.gorn && void window.gorn.session.reset()}
      >
        ⟲ {tr('Новая сессия', 'New session')}
      </button>
    </div>
  );
}
