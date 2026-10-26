import { tr } from '../../core/i18n.js';
import { RUN, pluralize } from '../../core/plural.js';
import type { Snapshot } from '../../main/engine.js';
import { num } from '../format.js';
import { Fe } from './Fe.js';
import { ItemIcon } from './ui.js';

/**
 * Сравнение стратегий.
 *
 * Стратегия определяется автоматически — по тому, чем открывалась карта.
 * Вложения известны поштучно из лога, поэтому размечать забеги руками
 * не нужно. Рядом с каждой цифрой стоит размер выборки: две стратегии
 * по три забега сравнивать бессмысленно, и это должно быть видно.
 */
export function Strategies({ snapshot }: { snapshot: Snapshot }) {
  const { strategies } = snapshot;
  if (strategies.length < 2) return null;

  // Лучшую отмечаем только среди тех, у кого выборка уже что-то значит.
  // Подсвечивать «1 136 в час» на двух забегах — ровно та ошибка,
  // против которой построен весь остальной интерфейс.
  const trustworthy = strategies.filter((s) => s.rate.confidence !== 'insufficient');
  const best = trustworthy.length > 1 ? Math.max(...trustworthy.map((s) => s.rate.perHour)) : null;

  return (
    <section className="card">
      <h2>{tr('Стратегии', 'Strategies')}</h2>
      <StrategyBars strategies={strategies} best={best} />
      <div className="strategies">
        {strategies.map((s) => {
          return (
            <div
              key={s.key}
              className={`strategy ${best !== null && s.rate.perHour === best ? 'best' : ''}`}
            >
              <div className="strategy-head">
                {s.costItems.length > 0 && (
                  <div className="strategy-icons">
                    {s.costItems.map((c) => (
                      <ItemIcon key={c.configBaseId} src={c.icon} />
                    ))}
                  </div>
                )}
                <span className="strategy-title" title={s.title}>
                  {s.title}
                </span>
              </div>

              <div className="strategy-rate">
                <Fe value={s.rate.perHour} />
                <span className="muted small">{tr('в час', 'per hour')}</span>
              </div>

              <div className="strategy-meta">
                <span className="muted">{pluralize(s.runs, RUN)}</span>
              </div>

              <div className="strategy-money">
                <span>
                  {tr('вложено', 'invested')} <Fe value={s.costPerRun} /> ·{' '}
                  {tr('чистыми', 'net')} <Fe value={s.netPerRun} />
                </span>
              </div>
            </div>
          );
        })}
      </div>
      <p className="muted small">
        {best === null
          ? tr(
              'Стратегия определяется автоматически по тому, чем открыта карта. Пока ни по одной не набралось данных для сравнения — нужно хотя бы 15 забегов.',
              'The strategy is detected automatically from what opened the map. None has enough data to compare yet — it takes at least 15 runs.',
            )
          : tr(
              'Стратегия определяется автоматически по тому, чем открыта карта. Сравнивайте только при сопоставимом числе забегов.',
              'The strategy is detected automatically from what opened the map. Only compare strategies with a similar number of runs.',
            )}
      </p>
    </section>
  );
}

/**
 * Сравнение стратегий одним взглядом: полосы «в час».
 * Честность про размер выборки закодирована в геометрию: «данных мало» —
 * тонкая полупрозрачная полоса, ей веришь ровно настолько, насколько видно.
 */
function StrategyBars({
  strategies,
  best,
}: {
  strategies: Snapshot['strategies'];
  best: number | null;
}) {
  const max = Math.max(...strategies.map((s) => s.rate.perHour));
  if (max <= 0) return null;
  const pct = (v: number) => `${Math.max(0, Math.min(100, (v / max) * 100))}%`;

  return (
    <div className="stratbars">
      {strategies.map((s) => {
        const weak = s.rate.confidence === 'insufficient';
        return (
          <div className="stratbar-row" key={s.key}>
            <span className="stratbar-title" title={s.title}>
              {s.title}
            </span>
            <div className="stratbar-track">
              <i
                className={`stratbar-bar ${weak ? 'stratbar-weak' : ''} ${
                  best !== null && s.rate.perHour === best ? 'stratbar-best' : ''
                }`}
                style={{ width: pct(s.rate.perHour) }}
              />
            </div>
            <span className="stratbar-value num">{num(s.rate.perHour)}</span>
          </div>
        );
      })}
    </div>
  );
}
