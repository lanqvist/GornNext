import { tr } from '../../core/i18n.js';
import { ITEM, pluralize } from '../../core/plural.js';
import type { Snapshot } from '../../main/engine.js';
import { num } from '../format.js';
import { Fe } from './Fe.js';
import { ItemIcon, Tip } from './ui.js';

/** Блок биржи. Реализованная выручка против бумажной оценки. */
export function Sidebar({ snapshot }: { snapshot: Snapshot }) {
  const { realizedFe, realizationRatio, listedPaperFe } = snapshot.totals;

  return (
    <aside className="card side">
      <h2>{tr('Торговый дом', 'Exchange')}</h2>
      {/* Обе строки про одно и то же — про выставленный товар. Ставить сюда
          оценку всей добычи нельзя: это несравнимая величина, а рядом
          она читается как отношение. */}
      <div className="side-row">
        <span>{tr('Выставлено на сумму', 'Listed value')}</span>
        <Fe value={listedPaperFe} />
      </div>
      <div className="side-row">
        <span>{tr('Реально пришло', 'Actually received')}</span>
        <Fe value={realizedFe} />
      </div>

      {/* Больше 100% бывает законно: выплаты приходят за лоты, выставленные
          до того, как трекер начал наблюдать. Это не ошибка, но выглядит
          как она, поэтому объясняем прямо. */}
      {realizationRatio !== null && realizationRatio > 1.2 && (
        <p className="muted small">
          {tr(
            'Пришло больше, чем оценено выставленное: часть выплат — за лоты, выставленные до начала наблюдения. Коэффициент станет осмысленным после нескольких полных сделок.',
            'More came in than the listings were valued at: some payouts are for listings made before tracking started. The ratio will mean something after a few complete sales.',
          )}
        </p>
      )}

      {realizationRatio !== null && realizationRatio <= 1.2 && (
        <>
          <div className="ratio">
            <div className="ratio-bar">
              <i style={{ width: `${Math.min(100, realizationRatio * 100)}%` }} />
            </div>
            <div className="ratio-value">{Math.round(realizationRatio * 100)}%</div>
          </div>
          <p className="muted small">
            {tr(
              'Столько реально дошло до кармана от того, во что оценивался выставленный товар. Разницу съедают комиссия биржи и продажа ниже референсной цены.',
              'This much of the listed value actually reached your pocket. The gap is the Exchange fee and selling below the reference price.',
            )}
          </p>
        </>
      )}
      {realizationRatio === null && (
        <p className="muted small">
          {tr(
            'Выстави что-нибудь на биржу и забери выручку — посчитаю, сколько реально доходит от бумажной оценки.',
            'List something on the Exchange and collect the proceeds — I’ll work out how much of the paper value actually comes through.',
          )}
        </p>
      )}
      <Bag snapshot={snapshot} />
    </aside>
  );
}

/**
 * Стоимость сумки по ценам — как нетворс у Exilence и TITrack.
 * ФЕ на руках отдельно: это деньги, а остальное — оценка, которую
 * ещё предстоит продать (и коэффициент выше говорит, за сколько).
 */
function Bag({ snapshot }: { snapshot: Snapshot }) {
  const bag = snapshot.bag;
  if (!bag) return null;
  return (
    <div className="bag">
      <h2>
        <Tip
          tip={tr(
            'Всё, что сейчас лежит в сумке, по тем же ценам, что и добыча. Сумка известна с последнего входа в игру.',
            'Everything in your bag right now, at the same prices as loot. The bag is known since your last login.',
          )}
        >
          {tr('Сумка', 'Bag')}
        </Tip>
      </h2>
      <div className="side-row">
        <span>{tr('Всё по ценам', 'Total value')}</span>
        <Fe value={bag.valueFe} />
      </div>
      <div className="side-row">
        <span>{tr('Из них ФЕ на руках', 'Of which FE in hand')}</span>
        <Fe value={bag.feCash} />
      </div>
      {bag.top.length > 0 && (
        <ul className="bag-top">
          {bag.top.map((item) => (
            <li key={item.configBaseId}>
              <ItemIcon src={item.icon} />
              <span className="bag-name" title={item.name}>
                {item.name}
              </span>
              <span className="muted small">×{num(item.qty)}</span>
              <Fe value={item.totalFe} />
            </li>
          ))}
        </ul>
      )}
      {bag.unpriced > 0 && (
        <p className="muted small">
          {tr(
            `Без цены ${pluralize(bag.unpriced, ITEM)} — оценка занижена на их стоимость.`,
            `${pluralize(bag.unpriced, ITEM)} without a price — the estimate is low by their value.`,
          )}
        </p>
      )}
    </div>
  );
}
