import { tr } from '../../core/i18n.js';
import { RUN, pluralize } from '../../core/plural.js';
import type { Snapshot } from '../../main/engine.js';
import { duration, num } from '../format.js';
import { Fe } from './Fe.js';
import { Tip } from './ui.js';

/** Меньше стольких забегов карта в сравнении «лучшая» не участвует. */
const MIN_RUNS = 5;

/**
 * Доходность по картам — «куда идти». Стратегии отвечают на «чем
 * открывать», а одни и те же вложения на разных картах дают разное.
 *
 * Карты с малой выборкой показываются, но приглушены и не могут стать
 * «лучшей»: два удачных забега — не повод менять маршрут.
 */
export function Maps({ snapshot }: { snapshot: Snapshot }) {
  const maps = snapshot.maps;
  if (maps.length < 2) return null;

  const comparable = maps.filter((m) => m.runs >= MIN_RUNS);
  const best =
    comparable.length >= 2
      ? comparable.reduce((a, b) => (b.rate.median > a.rate.median ? b : a))
      : null;
  const top = Math.max(...maps.map((m) => m.rate.p75), 1);

  return (
    <section className="card">
      <h2>{tr('Карты', 'Maps')}</h2>
      <table className="runs maps">
        <thead>
          <tr>
            <th>{tr('Карта', 'Map')}</th>
            <th className="r">{tr('Забегов', 'Runs')}</th>
            <th>
              <Tip
                tip={tr(
                  'Медиана чистыми в час; полоса — обычный разброс P25–P75.',
                  'Median net per hour; the bar shows the typical range (P25–P75).',
                )}
              >
                {tr('В час', 'Per hour')}
              </Tip>
            </th>
            <th className="r">{tr('За карту', 'Per map')}</th>
            <th className="r">{tr('Время карты', 'Map time')}</th>
          </tr>
        </thead>
        <tbody>
          {maps.map((m) => {
            const weak = m.runs < MIN_RUNS;
            return (
              <tr key={m.name} className={weak ? 'map-weak' : ''}>
                <td>
                  {m.name}
                  {m === best && (
                    <span className="badge badge-good map-best">{tr('лучшая', 'best')}</span>
                  )}
                </td>
                <td className="r muted">
                  {weak ? (
                    <Tip
                      tip={tr(
                        `Меньше ${MIN_RUNS} забегов — сравнивать рано.`,
                        `Fewer than ${MIN_RUNS} runs: too early to compare.`,
                      )}
                    >
                      {m.runs}
                    </Tip>
                  ) : (
                    m.runs
                  )}
                </td>
                <td>
                  <div className="map-rate">
                    <span className="map-rate-value">{num(m.rate.median)}</span>
                    <span
                      className="map-track"
                      role="img"
                      aria-label={tr(
                        `разброс ${num(m.rate.p25)}–${num(m.rate.p75)}, медиана ${num(m.rate.median)}`,
                        `typical range ${num(m.rate.p25)}–${num(m.rate.p75)}, median ${num(m.rate.median)}`,
                      )}
                    >
                      <span
                        className="map-band"
                        style={{
                          left: `${(Math.max(0, m.rate.p25) / top) * 100}%`,
                          width: `${(Math.max(0, m.rate.p75 - Math.max(0, m.rate.p25)) / top) * 100}%`,
                        }}
                      />
                      <span
                        className="map-median"
                        style={{ left: `${(Math.max(0, m.rate.median) / top) * 100}%` }}
                      />
                    </span>
                  </div>
                </td>
                <td className="r">
                  <Fe value={m.netPerRun} />
                </td>
                <td className="r muted">{duration(m.avgActiveMs)}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
      <p className="muted small">
        {tr(
          'Карта — по внутреннему имени из лога: русских названий игра в лог не пишет. Сравнение «лучшая» — среди карт, где набралось хотя бы',
          'Maps are named by their internal asset names: that is all the game writes to the log. The “best” comparison only covers maps with at least',
        )}{' '}
        {pluralize(MIN_RUNS, RUN)}.
      </p>
    </section>
  );
}
