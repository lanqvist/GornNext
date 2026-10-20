import { tr } from '../../core/i18n.js';
import { RUN_IN, pluralize } from '../../core/plural.js';
import type { Snapshot } from '../../main/engine.js';
import { num } from '../format.js';
import { CopyName, ItemIcon } from './ui.js';

/**
 * Что пробить следующим.
 *
 * Цена берётся из твоих собственных пробивок в Торговом доме, и каждая
 * стоит одного клика. Поэтому важно не количество предметов без цены,
 * а какие из них реально портят оценку.
 */
export function MissingPrices({ snapshot }: { snapshot: Snapshot }) {
  const { missing } = snapshot;
  if (missing.length === 0) return null;

  return (
    <section className="card">
      <h2>{tr('Пробить цену', 'Check the price')}</h2>
      <p className="muted small">
        {tr(
          'Эти предметы падают, но стоимость неизвестна — на них оценка добычи занижена. Порядок по влиянию: сверху то, чего выпало больше всего. Мусор, который не продаётся, можно не учитывать — он перестанет сюда попадать.',
          'These items drop, but their value is unknown, so your loot estimate is low by that much. Sorted by impact: what dropped the most is on top. Junk that doesn’t sell can be ignored — it will stop showing up here.',
        )}
      </p>
      <ul className="missing">
        {missing.map((m) => (
          <li key={m.configBaseId}>
            <ItemIcon src={m.icon} />
            <CopyName name={m.name} />
            <span className="drop-qty">{tr(`${num(m.qty)} шт`, `×${num(m.qty)}`)}</span>
            <span className="muted small">
              {tr('в', 'in')} {pluralize(m.runs, RUN_IN)}
            </span>
            <button
              className="link-button"
              disabled={!window.gorn}
              title={tr(
                'Считать предмет бесплатным во всех цифрах. Вернуть можно в блоке «Цены».',
                'Count the item as worthless in every number. You can restore it in the Prices block.',
              )}
              onClick={() => void window.gorn?.prices.setExcluded(m.configBaseId, true)}
            >
              {tr('не учитывать', 'ignore')}
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}
