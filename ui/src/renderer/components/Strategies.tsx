import { tr } from '../../core/i18n.js';
import type { Snapshot } from '../../main/engine.js';
import { num } from '../format.js';
import { Fe } from './Fe.js';
import { ItemIcon, Tip } from './ui.js';

/**
 * Рейтинг стратегий — одним видом: стратегия → в час, забеги, чистыми
 * и вложено за карту. Раньше те же цифры шли дважды: полосами и
 * карточками.
 *
 * Стратегия определяется автоматически — по тому, чем открывалась карта.
 * Честность про размер выборки — в геометрии: «данных мало» — строка
 * приглушена и полоса тонкая, лучшей она стать не может.
 */
export function Strategies({ snapshot }: { snapshot: Snapshot }) {
  const { strategies } = snapshot;
  if (strategies.length < 2) return null;

  // Лучшую отмечаем только среди тех, у кого выборка уже что-то значит.
  // Подсвечивать «1 136 в час» на двух забегах — ровно та ошибка,
  // против которой построен весь остальной интерфейс.
  const trustworthy = strategies.filter((s) => s.rate.confidence !== 'insufficient');
  const best = trustworthy.length > 1 ? Math.max(...trustworthy.map((s) => s.rate.perHour)) : null;
  const top = Math.max(...strategies.map((s) => s.rate.perHour), 1);
  const ranked = [...strategies].sort((a, b) => b.rate.perHour - a.rate.perHour);

  return (
    <section className="card">
      <h2>
        <Tip
          tip={tr(
            'Стратегия определяется автоматически — по тому, чем открыта карта. Сравнивать стоит при сопоставимом числе забегов; «лучшая» выбирается только среди стратегий, где набралось хотя бы 15 забегов.',
            'The strategy is detected automatically from what opened the map. Compare strategies with a similar number of runs; “best” is only picked among strategies with at least 15 runs.',
          )}
        >
          {tr('Стратегии', 'Strategies')}
        </Tip>
      </h2>
      <table className="runs strats">
        <thead>
          <tr>
            <th>{tr('Чем открывали', 'Opened with')}</th>
            <th className="r">{tr('Забегов', 'Runs')}</th>
            <th>
              <Tip
                tip={tr(
                  'Все забеги стратегии вместе: добыча за вычетом вложений ÷ время на картах.',
                  'All runs of the strategy together: loot net of costs ÷ time in maps.',
                )}
              >
                {tr('В час', 'Per hour')}
              </Tip>
            </th>
            <th className="r">{tr('Чистыми за карту', 'Net per map')}</th>
            <th className="r">{tr('Вложено за карту', 'Cost per map')}</th>
          </tr>
        </thead>
        <tbody>
          {ranked.map((s) => {
            const weak = s.rate.confidence === 'insufficient';
            const isBest = best !== null && s.rate.perHour === best;
            return (
              <tr key={s.key} className={weak ? 'map-weak' : ''}>
                <td>
                  <span className="strat-name">
                    {s.costItems.length > 0 && (
                      <span className="strategy-icons">
                        {s.costItems.map((c) => (
                          <ItemIcon key={c.configBaseId} src={c.icon} />
                        ))}
                      </span>
                    )}
                    <span className="strategy-title" title={s.title}>
                      {s.title}
                    </span>
                    {isBest && (
                      <span className="badge badge-good map-best">{tr('лучшая', 'best')}</span>
                    )}
                  </span>
                </td>
                <td className="r muted">
                  {weak ? (
                    <Tip
                      tip={tr(
                        'Данных мало — нужно хотя бы 15 забегов, чтобы сравнивать.',
                        'Not enough data: it takes at least 15 runs to compare.',
                      )}
                    >
                      {s.runs}
                    </Tip>
                  ) : (
                    s.runs
                  )}
                </td>
                <td>
                  <div className="map-rate">
                    <span className="map-rate-value">{num(s.rate.perHour)}</span>
                    <span className="map-track" aria-hidden="true">
                      <span
                        className={`map-band ${isBest ? 'map-band-best' : ''}`}
                        style={{ left: 0, width: `${(Math.max(0, s.rate.perHour) / top) * 100}%` }}
                      />
                    </span>
                  </div>
                </td>
                <td className="r">
                  <Fe value={s.netPerRun} />
                </td>
                <td className="r muted">
                  <Fe value={s.costPerRun} />
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </section>
  );
}
