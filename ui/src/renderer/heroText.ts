import { tr } from '../core/i18n.js';
import { RUN, pluralize } from '../core/plural.js';

/**
 * Пояснение к широкому разбросу. Обещание «станет устойчивой после N
 * забегов» уместно, только пока забегов меньше N: при 627 забегах оно
 * звучало как поломка. Дальше разброс — свойство фарма, а не нехватка
 * данных, и подсказка говорит, где искать его причину.
 */
export function spreadText(sampleSize: number, wide: boolean): string | null {
  if (sampleSize === 0 || !wide) return null;
  const settleAt = sampleSize < 15 ? 15 : sampleSize < 50 ? 50 : null;
  if (settleAt !== null) {
    return tr(
      `Забеги сильно различаются: разброс шире самой медианы. Цифра станет устойчивой после ${pluralize(settleAt, RUN)}.`,
      `Runs vary a lot: the spread is wider than the median itself. The number will settle after ${pluralize(settleAt, RUN)}.`,
    );
  }
  return tr(
    'Забеги сильно различаются: разброс шире самой медианы. Откуда разница — видно в стратегиях и картах ниже.',
    'Runs vary a lot: the spread is wider than the median itself. Strategies and maps below show where the difference comes from.',
  );
}
