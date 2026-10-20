import { tr } from '../../core/i18n.js';
import { RUN, VISIT, pluralize } from '../../core/plural.js';
import { NORM_WINDOW } from '../../core/analysis.js';
import type { Snapshot } from '../../main/engine.js';
import { NBSP, cacheDate, duration, num, time } from '../format.js';
import { Fe } from './Fe.js';
import { spreadText } from '../heroText.js';
import { ConfidenceBadge, Tile, Tip } from './ui.js';

/**
 * Спарклайн темпа: последние забеги в хронологии. Полоса — P25–P75
 * («обычный разброс»), пунктир — медиана, точка — самый свежий забег.
 * Один взгляд отвечает на «расту я или проседаю» лучше таблицы.
 */
function RateSparkline({ snapshot }: { snapshot: Snapshot }) {
  const runs = [...snapshot.recent].reverse();
  if (runs.length < 5) return null;

  const values = runs.map((r) => r.perHour);
  const { p25, p75, median } = snapshot.rate;
  const w = 300;
  const h = 54;
  const pad = 5;
  const max = Math.max(...values, p75);
  const min = Math.min(...values, p25, 0);
  const span = max - min || 1;
  const x = (i: number) => pad + (i / (values.length - 1)) * (w - pad * 2);
  const y = (v: number) => h - pad - ((v - min) / span) * (h - pad * 2);
  const points = values.map((v, i) => `${x(i)},${y(v)}`).join(' ');
  const last = values.length - 1;

  return (
    <div className="spark">
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
        {/* Обычный разброс: половина забегов внутри этой полосы. */}
        <rect
          x={pad}
          y={y(p75)}
          width={w - pad * 2}
          height={Math.max(2, y(p25) - y(p75))}
          className="spark-band"
        />
        <line x1={pad} x2={w - pad} y1={y(median)} y2={y(median)} className="spark-median" />
        <polyline points={points} className="spark-line" />
        <circle cx={x(last)} cy={y(values[last]!)} r={3} className="spark-dot" />
        {/* Невидимые увеличенные цели наведения: подсказка на каждую точку. */}
        {values.map((v, i) => (
          <circle key={i} cx={x(i)} cy={y(v)} r={7} fill="transparent">
            <title>
              {`${time(runs[i]!.startedAt)} · ${runs[i]!.name}: ${num(v)} ${tr('ФЕ/час', 'FE/h')}`}
            </title>
          </circle>
        ))}
      </svg>
      <span className="spark-caption muted small">
        {tr('темп последних', 'pace of the last')} {pluralize(values.length, RUN)}
      </span>
    </div>
  );
}

/**
 * Главный блок. Крупно — медиана последних забегов: «как идёт сейчас».
 * Медиана всего сезона копит ранние слабые недели (412 в час при сессии
 * в 1 446) и отвечает на другой вопрос — она стоит ниже, как норма.
 * Среднее рядом мелко — сознательно: в ARPG оно задирается одной редкой
 * находкой и описывает забег, которого не бывает.
 */
export function Hero({ snapshot }: { snapshot: Snapshot }) {
  const { rate, feRate, totals } = snapshot;
  const spread = rate.p75 - rate.p25;
  // «Вложено 0» при потраченном маяке выглядит как поломка. Причина обычно
  // одна: у расходника нет цены, и вычесть нечего.
  const unpricedCost = snapshot.missing.find((m) => m.isCost);

  // norm — медиана последних 20 забегов (как у сравнений «к норме»).
  const recentCount = Math.min(NORM_WINDOW, rate.sampleSize);
  const now = rate.sampleSize > 0 ? snapshot.norm : 0;
  const vsSeason =
    rate.sampleSize > NORM_WINDOW && rate.median > 0
      ? Math.round(((now - rate.median) / rate.median) * 100)
      : null;
  const spreadNote = spreadText(rate.sampleSize, spread > rate.median * 0.8);

  return (
    <section className="hero">
      <div className="hero-main">
        <div className="hero-label">
          <Tip
            tip={tr(
              'Оценка всей добычи по ценам (свои пробивки + база сообщества) минус вложения в карты. Медиана последних 20 забегов: показывает, как идёт сейчас, и устойчива к одной редкой дорогой находке.',
              'All loot valued at prices (your own price checks + community prices) minus map costs. Median of the last 20 runs: shows how things go right now and holds steady against a single rare expensive drop.',
            )}
          >
            {tr('Добыча в час сейчас, за вычетом вложений', 'Loot per hour now, net of map costs')}
          </Tip>
        </div>
        <div className="hero-value">
          <Fe value={now} />
          {vsSeason !== null && (
            <span
              className={`hero-vs ${vsSeason >= 0 ? 'up' : 'down'}`}
              title={tr('Последние забеги к медиане сезона', 'Recent runs vs the season median')}
            >
              {vsSeason === 0
                ? tr('на уровне сезона', 'on par with the season')
                : `${vsSeason > 0 ? '↑' : '↓'} ${Math.abs(vsSeason)}% ${tr('к сезону', 'vs season')}`}
            </span>
          )}
        </div>
        <div className="hero-sub">
          <span className="muted">
            {tr('медиана · последние', 'median · last')} {pluralize(recentCount, RUN)}
          </span>
          <Tip
            tip={tr(
              'Медиана по всем забегам сезона (или выбранного персонажа) — норма, с которой сравниваются забеги.',
              'Median across all runs of the season (or the chosen character) — the norm runs are compared against.',
            )}
          >
            <span>
              {tr('сезон', 'season')} <Fe value={rate.median} /> · {pluralize(rate.sampleSize, RUN)}
            </span>
          </Tip>
          <ConfidenceBadge level={rate.confidence} />
          <Tip
            tip={tr(
              'P25–P75: половина забегов укладывается в эти границы.',
              'P25–P75: half of all runs fall within this range.',
            )}
          >
            <span className="muted">
              {tr('обычный разброс', 'typical range')} {num(rate.p25)}–{num(rate.p75)}
            </span>
          </Tip>
          <Tip
            tip={tr(
              'Среднее задирается одной редкой находкой и описывает забег, которого не бывает. Решения принимайте по медиане.',
              'The mean gets inflated by a single rare drop and describes a run that never happens. Make decisions by the median.',
            )}
          >
            <span className="muted">
              {tr('среднее', 'mean')} {num(rate.mean)}
            </span>
          </Tip>
        </div>
        {spreadNote && <p className="note">{spreadNote}</p>}
        {rate.topShare > 0.3 && rate.sampleSize > 1 && (
          <p className="note warn">
            {tr(
              `Один забег дал ${Math.round(rate.topShare * 100)}% всей добычи — среднее держится на нём, медиана нет.`,
              `A single run brought in ${Math.round(rate.topShare * 100)}% of all loot — the mean rests on it, the median doesn't.`,
            )}
          </p>
        )}
        <RateSparkline snapshot={snapshot} />
      </div>

      <div className="hero-side">
        <div className="hero-tiles">
          {/* Накопительный темп: двигается при подборе, медиана — намеренно нет.
              Подпись «за сессию» важна, иначе медленное сползание между
              подборами выглядит как ошибка, а не как рост знаменателя. */}
          <Tile
            label={tr('За сессию, в час', 'This session, per hour')}
            tip={tr(
              'Накопительный темп: вся добыча сессии ÷ активное время в картах. Двигается при каждом подборе, между подборами медленно сползает — это рост знаменателя, не поломка.',
              'Cumulative pace: all session loot ÷ active time in maps. It moves with every pickup and slowly drifts down between pickups — that is the denominator growing, not a bug.',
            )}
            value={<Fe value={snapshot.liveRate} />}
            live
          />
          <Tile
            label={tr('Чистый ФЕ в час', 'Raw FE per hour')}
            tip={tr(
              'Только сама валюта ФЕ, упавшая в сумку, — без оценки предметов по ценам.',
              'Only the FE currency itself that landed in your bag — no item valuation.',
            )}
            value={<Fe value={feRate.median} />}
          />
          <Tile
            label={tr('На картах', 'In maps')}
            tip={tr(
              'Активное время за сессию: паузы дольше трёх минут не идут в зачёт.',
              "Active time this session: pauses longer than three minutes don't count.",
            )}
            value={duration(totals.inMapMs)}
          />
          <Tile
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
        </div>
        {snapshot.prison.runs > 0 && (
          <Tip
            tip={tr(
              'Тюрьма Талоса входит в итог сессии, но не в темп, медианы, рекорды, карты и стратегии: игра на доске идёт вне карт, и её время лог не видит.',
              'Tartarus Prison counts toward the session total but not the pace, medians, records, maps or strategies: the board is played outside maps, and the log cannot see that time.',
            )}
          >
            <span className="hero-prison">
              {tr('тюрьма', 'prison')}: <Fe value={snapshot.prison.netFe} signed />{' '}
              {tr('за', 'in')} {pluralize(snapshot.prison.runs, VISIT)}
            </span>
          </Tip>
        )}
        <SessionBar snapshot={snapshot} />
      </div>
    </section>
  );
}

/**
 * Границы сессии: с какого момента считают плитки, и кнопка начать заново.
 *
 * Сброс ничего не удаляет — прошлые забеги остаются в истории и медиане,
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
      <span className="muted small">
        {startedAt > 0
          ? tr(`сессия с ${started}`, `session since ${started}`)
          : tr('сессия — вся история', 'session — all history')}
        {previous && (
          <>
            {' '}
            · {tr('прошлая:', 'previous:')} {previous.netFe >= 0 ? '+' : ''}
            {num(previous.netFe)}
            {NBSP}
            {tr('ФЕ за', 'FE in')} {duration(previous.inMapMs)}
          </>
        )}
      </span>
      <button
        className="session-reset"
        title={tr(
          'Плитки и цель оверлея начнут отсчёт заново. История, медиана и стратегии никуда не денутся.',
          'The tiles and the overlay goal will start counting from scratch. History, median and strategies stay put.',
        )}
        onClick={() => window.gorn && void window.gorn.session.reset()}
      >
        ⟲ {tr('Новая сессия', 'New session')}
      </button>
    </div>
  );
}
