import { tr } from '../../core/i18n.js';
import { ITEM, pluralize } from '../../core/plural.js';
import type { RunView } from '../../main/engine.js';
import { num } from '../format.js';
import { tierOf, useTierContext } from '../tiers.js';
import { Fe } from './Fe.js';
import { CopyName, ItemIcon } from './ui.js';

// Функция, а не константа: на уровне модуля tr() сработал бы до выбора языка.
const communityTip = () =>
  tr(
    'Цена из базы сообщества (titrack.ninja): своей пробивки нет или ей больше трёх дней. Пробей предмет сам — свежая своя пробивка точнее и заменит её.',
    'Price from community prices (titrack.ninja): you have no own price check, or it is over three days old. Check the item yourself — a fresh own price check is more accurate and will replace it.',
  );

/**
 * Список добычи. Дешевле порога `minFe` и исключённое из учёта
 * сворачивается в одну строку: двадцать строк песка по 0,01 ФЕ
 * прятали за собой то, что действительно выпало.
 */
export function DropList({
  drops,
  empty = tr('Ничего не выпало.', 'Nothing dropped.'),
  minFe = 0,
}: {
  drops: RunView['drops'];
  empty?: string;
  minFe?: number;
}) {
  if (drops.length === 0) return <p className="muted">{empty}</p>;

  const excluded = drops.filter((d) => d.priceSource === 'excluded');
  const small = drops.filter(
    (d) => d.priceSource !== 'excluded' && d.totalFe !== null && d.totalFe < minFe,
  );
  const shown = drops.filter((d) => !excluded.includes(d) && !small.includes(d));
  const smallFe = small.reduce((a, d) => a + (d.totalFe ?? 0), 0);
  // Ступени — те же, что в оверлее и всплывашках: дорогое видно сразу.
  const tiers = useTierContext();

  return (
    <ul className="drops">
      {shown.slice(0, 12).map((d) => (
        <li key={d.configBaseId} className={`t-${tierOf(d.totalFe, tiers) ?? 'none'}`}>
          <ItemIcon src={d.icon} />
          <CopyName name={d.name} />
          <span className="drop-qty">×{num(d.qty)}</span>
          <span className="drop-value">
            {d.totalFe === null ? (
              <span
                className="muted"
                title={tr(
                  'Цена неизвестна — пробей предмет в Торговом доме',
                  'Price unknown — check the item on the Exchange',
                )}
              >
                {tr('без цены', 'no price')}
              </span>
            ) : (
              <span
                {...(d.priceSource === 'community'
                  ? { className: 'price-community', title: communityTip() }
                  : {})}
              >
                <Fe value={d.totalFe} />
              </span>
            )}
          </span>
        </li>
      ))}
      {small.length > 0 && (
        <li className="drops-folded">
          <span className="no-icon" />
          <span className="muted">
            {tr(
              `ещё ${pluralize(small.length, ITEM)} дешевле ${num(minFe, minFe < 1 ? 1 : 0)} ФЕ`,
              `${pluralize(small.length, ITEM)} more under ${num(minFe, minFe < 1 ? 1 : 0)} FE`,
            )}
          </span>
          <span className="drop-qty" />
          <span className="drop-value muted">
            <Fe value={smallFe} />
          </span>
        </li>
      )}
      {excluded.length > 0 && (
        <li className="drops-folded">
          <span className="no-icon" />
          <span
            className="muted"
            title={tr(
              `Исключено из учёта: ${excluded.map((d) => d.name).join(', ')}. Вернуть — в блоке «Цены».`,
              `Ignored: ${excluded.map((d) => d.name).join(', ')}. Restore them in the Prices block.`,
            )}
          >
            {tr(
              `не учитывается: ${pluralize(excluded.length, ITEM)}`,
              `ignored: ${pluralize(excluded.length, ITEM)}`,
            )}
          </span>
        </li>
      )}
    </ul>
  );
}
