import { tr } from '../../core/i18n.js';
import { RUN, pluralize } from '../../core/plural.js';
import type { Snapshot } from '../../main/engine.js';
import { num } from '../format.js';
import { Fe } from './Fe.js';

/**
 * Рекорды по последним забегам: дешёвая радость без гринд-давления.
 * Живут строкой под графиком темпа — справа от нормы и сезона.
 * Всё считается из среза на клиенте — движку эта полоска не стоит ничего.
 */
export function SessionRecords({ snapshot }: { snapshot: Snapshot }) {
  const runs = snapshot.recent;
  if (runs.length < 5) return null;

  const best = runs.reduce((a, r) => (r.perHour > a.perHour ? r : a));

  // Стрик: сколько свежих забегов подряд держатся выше нормы.
  let streak = 0;
  if (snapshot.norm > 0) {
    for (const r of runs) {
      if (r.perHour >= snapshot.norm) streak++;
      else break;
    }
  }

  let find: { name: string; fe: number } | null = null;
  for (const r of runs) {
    for (const d of r.drops) {
      if (d.unitFe !== null && (find === null || d.unitFe > find.fe)) {
        find = { name: d.name, fe: d.unitFe };
      }
    }
  }

  return (
    <span className="records">
      <span>
        {tr('лучший', 'best')}{' '}
        <b>
          {num(best.perHour)} {tr('ФЕ/час', 'FE/h')}
        </b>{' '}
        {best.name}
      </span>
      {find && (
        <span>
          {tr('дорогая находка', 'top drop')} <b>{find.name}</b> <Fe value={find.fe} />
        </span>
      )}
      {streak >= 2 && (
        <span className="records-streak">
          {tr(`${pluralize(streak, RUN)} подряд выше обычного`, `${pluralize(streak, RUN)} in a row above usual`)}
        </span>
      )}
    </span>
  );
}
