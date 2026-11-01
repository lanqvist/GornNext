import { useState } from 'react';

import { tr } from '../../core/i18n.js';
import { ITEM, RUN_IN, pluralize } from '../../core/plural.js';
import type { Snapshot } from '../../main/engine.js';
import { num } from '../format.js';
import { CopyName, ItemIcon } from './ui.js';

/** Предмет вне справочника: движок называет его по номеру, «id 260005». */
const isUnnamed = (name: string) => /^id \d+$/.test(name);

/**
 * Тихая строка под сводкой: часть добычи без цены, оценка занижена.
 *
 * Раньше это была вкладка «Пробить цену», где большая часть строк —
 * «id 260005»: игрок не знает, что это, и пробить не может. Теперь —
 * одна строка, по нажатию короткий список с названиями и «не учитывать».
 * Безымянное в счёт не идёт: его нет в справочнике, это обычно
 * непродаваемое (так же его считает оценка сумки), — оно сведено
 * в одну строку списка. Нет названных предметов без цены — нет и строки.
 */
export function MissingPrices({ snapshot }: { snapshot: Snapshot }) {
  const [open, setOpen] = useState(false);
  const { missing } = snapshot;
  if (missing.length === 0) return null;

  const named = missing.filter((m) => !isUnnamed(m.name));
  const unnamed = missing.length - named.length;
  if (named.length === 0) return null;

  return (
    <div className="missing-note">
      <button className="missing-toggle" aria-expanded={open} onClick={() => setOpen(!open)}>
        <i aria-hidden="true" />
        {tr(
          `${pluralize(named.length, ITEM)} без цены — оценка может быть занижена`,
          `${pluralize(named.length, ITEM)} without a price — the estimate may be low`,
        )}
        <span className="missing-chevron" aria-hidden="true">
          {open ? '▾' : '▸'}
        </span>
      </button>
      {open && (
        <ul className="missing">
          {named.map((m) => (
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
                  'Считать предмет бесплатным во всех цифрах — для того, что не продаётся. Вернуть: Настройки → Данные → Цены предметов.',
                  'Count the item as worthless in every number, for things that don’t sell. Restore it in Settings → Data → Item prices.',
                )}
                onClick={() => void window.gorn?.prices.setExcluded(m.configBaseId, true)}
              >
                {tr('не учитывать', 'ignore')}
              </button>
            </li>
          ))}
          {unnamed > 0 && (
            <li className="missing-unnamed">
              <span className="no-icon" />
              <span className="muted">
                {tr(
                  `и ещё ${pluralize(unnamed, ITEM)} без названия — их нет в справочнике, обычно это непродаваемое`,
                  `and ${unnamed} more without a name — not in the item list, usually things that don’t sell`,
                )}
              </span>
            </li>
          )}
          <li className="missing-hint muted small">
            {tr(
              'Цена появится, когда пробьёшь предмет в Торговом доме. Сверху — то, чего выпало больше всего.',
              'A price appears once you check the item on the Exchange. What dropped the most is on top.',
            )}
          </li>
        </ul>
      )}
    </div>
  );
}
