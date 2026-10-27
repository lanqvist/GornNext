import { tr } from '../../core/i18n.js';
import { RUN, VISIT, pluralize } from '../../core/plural.js';
import { NORM_WINDOW } from '../../core/analysis.js';
import type { Snapshot } from '../../main/engine.js';
import { NBSP, cacheDate, duration, num, time } from '../format.js';
import { Fe } from './Fe.js';
import { Tile, Tip } from './ui.js';

/**
 * Спарклайн темпа: последние забеги в хронологии. Пунктир — в час за
 * последние 20 забегов (норма), точка — самый свежий забег.
 * Один взгляд отвечает на «расту я или проседаю» лучше таблицы.
 */
function RateSparkline({ snapshot }: { snapshot: Snapshot }) {
  const runs = [...snapshot.recent].reverse();
  if (runs.length < 5) return null;

  const values = runs.map((r) => r.perHour);
  const norm = snapshot.norm;
  const w = 300;
  const h = 54;
  const pad = 5;
  const max = Math.max(...values, norm);
  const min = Math.min(...values, norm, 0);
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
        <line x1={pad} x2={w - pad} y1={y(norm)} y2={y(norm)} className="spark-median" />
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
 * Главный блок. Крупно — сколько выходит в час за сессию: вся добыча
 * за вычетом вложений ÷ время в картах, вместе с текущим забегом.
 * Ниже — то же «в час» за последние 20 забегов и за весь сезон.
 * Медианы здесь нет: она описывает «типичную карту», а не сколько
 * на деле приносит час фарма, — и расходилась с итогом сессии.
 *
 * Главная цифра — как у других трекеров, по зонам: итог сессии (карты,
 * бои Vorax и тюрьма) ÷ активное время в них; город не в счёт. Под ней —
 * сама формула, чтобы цифру можно было проверить в уме.
 */
export function Hero({ snapshot }: { snapshot: Snapshot }) {
  const { rate, feRate, totals } = snapshot;
  // «Вложено 0» при потраченном маяке выглядит как поломка. Причина обычно
  // одна: у расходника нет цены, и вычесть нечего.
  const unpricedCost = snapshot.missing.find((m) => m.isCost);
  const recentCount = Math.min(NORM_WINDOW, rate.sampleSize);
  const sessionNet = totals.valueFe - totals.costFe;
  const zoneMs = totals.inMapMs + totals.prisonMs;

  return (
    <section className="hero">
      <div className="hero-main">
        <div className="hero-label">
          <Tip
            tip={tr(
              'Как у других трекеров — по зонам: вся добыча сессии по ценам минус вложения (карты, бои Vorax и тюрьма), делённая на активное время в них. Город не в счёт, пауза на карте дольше 3 минут — тоже; тюрьма и Vorax — целиком, от входа в их окно до выхода из зоны. Сессия начинается с запуском игры или кнопкой «Новая сессия».',
              'Like other trackers — by zone: all loot this session at prices minus map costs (maps, Vorax fights and prison), divided by active time in them. Town doesn’t count, nor do pauses longer than 3 minutes in a map; prison and Vorax count whole, from opening their window to leaving the zone. A session starts when the game launches or with the “New session” button.',
            )}
          >
            {tr('В час за сессию, за вычетом вложений', 'Per hour this session, net of map costs')}
          </Tip>
        </div>
        <div className="hero-value">
          <Fe value={snapshot.liveRate} />
        </div>
        <div className="hero-formula">
          = <Fe value={sessionNet} signed /> {tr('за', 'in')} {duration(zoneMs)}
        </div>
        <div className="hero-sub">
          <Tip
            tip={tr(
              'Только время на картах, без города: последние забеги вместе — их добыча за вычетом вложений ÷ их время. С этим темпом сравниваются карты.',
              'Map time only, no town: the latest runs together — their loot net of costs ÷ their time. Maps are compared against this pace.',
            )}
          >
            <span>
              {tr('на картах: последние', 'in maps: last')} {pluralize(recentCount, RUN)} —{' '}
              <Fe value={snapshot.norm} /> {tr('в час', 'per hour')}
            </span>
          </Tip>
          <Tip
            tip={tr(
              'Все забеги сезона вместе: добыча за вычетом вложений ÷ время в картах, без города.',
              'All runs of the season together: loot net of costs ÷ time in maps, no town.',
            )}
          >
            <span className="muted">
              {tr('сезон', 'season')}: <Fe value={rate.perHour} /> {tr('в час', 'per hour')} ·{' '}
              {pluralize(rate.sampleSize, RUN)}
            </span>
          </Tip>
        </div>
        <RateSparkline snapshot={snapshot} />
      </div>

      <div className="hero-side">
        <div className="hero-tiles">
          <Tile
            label={tr('Итог сессии', 'Session total')}
            tip={tr(
              'Вся добыча сессии по ценам минус вложения в карты, вместе с тюрьмой.',
              'All loot this session at prices minus map costs, prison included.',
            )}
            value={<Fe value={sessionNet} signed />}
            live
          />
          <Tile
            label={tr('Чистый ФЕ в час', 'Raw FE per hour')}
            tip={tr(
              'Только сама валюта ФЕ, упавшая в сумку, — без оценки предметов по ценам. За сезон: вся ФЕ ÷ всё время в картах.',
              'Only the FE currency itself that landed in your bag — no item valuation. Season-wide: all FE ÷ all time in maps.',
            )}
            value={<Fe value={feRate.perHour} />}
          />
          <Tile
            label={tr('На картах', 'In maps')}
            tip={tr(
              `Активное время на картах за сессию: пауза дольше 3 минут не в счёт. Всего сессия идёт ${duration(totals.sessionMs)} — с городом и тюрьмой.`,
              `Active time in maps this session: pauses longer than 3 minutes don't count. The whole session has run ${duration(totals.sessionMs)}, town and prison included.`,
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
              'Тюрьма Талоса входит в итог сессии и в «в час» вместе со своим временем, но не в темп карт, рекорды, карты и стратегии.',
              'Tartarus Prison counts toward the session total and per hour together with its time, but not map pace, records, maps or strategies.',
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
          'Темп сессии, плитки и цель оверлея начнут отсчёт заново. История, сезон и стратегии никуда не денутся.',
          'The session pace, tiles and the overlay goal will start counting from scratch. History, season and strategies stay put.',
        )}
        onClick={() => window.gorn && void window.gorn.session.reset()}
      >
        ⟲ {tr('Новая сессия', 'New session')}
      </button>
    </div>
  );
}
