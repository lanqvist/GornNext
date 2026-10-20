/**
 * Русские склонения после числительных.
 *
 * В русском четыре формы, а не две: 1 забег, 2 забега, 5 забегов, 1,5 забега.
 * Наивное «одна форма или множественное число» даёт «2 забегов» — сразу видно,
 * что интерфейс переведён механически.
 */
import { uiLanguage } from './i18n.js';

const rules = new Intl.PluralRules('ru-RU');

export interface PluralForms {
  one: string;
  few: string;
  many: string;
  /** Английские формы: единственное и множественное. */
  en: readonly [string, string];
}

export function plural(count: number, forms: PluralForms): string {
  // В английском две формы, и «1» — единственная, где нужно единственное.
  if (uiLanguage() === 'en') return count === 1 ? forms.en[0] : forms.en[1];
  switch (rules.select(count)) {
    case 'one':
      return forms.one;
    case 'few':
      return forms.few;
    default:
      // «many» для 5–20, «other» для дробных — обе берут родительный падеж.
      return rules.select(count) === 'many' ? forms.many : forms.few;
  }
}

export const RUN = { one: 'забег', few: 'забега', many: 'забегов', en: ['run', 'runs'] } as const;
/**
 * Предложный падеж: «в 1 забеге», «в 2 забегах», «в 5 забегах».
 * Родительные формы после предлога «в» дают «в 5 забегов» — заметная калька.
 */
export const RUN_IN = { one: 'забеге', few: 'забегах', many: 'забегах', en: ['run', 'runs'] } as const;
export const ITEM = {
  one: 'предмет',
  few: 'предмета',
  many: 'предметов',
  en: ['item', 'items'],
} as const;
export const FIND = { one: 'находка', few: 'находки', many: 'находок', en: ['find', 'finds'] } as const;
/** Заходы в тюрьму: «за 4 захода», «за 11 заходов». */
export const VISIT = {
  one: 'заход',
  few: 'захода',
  many: 'заходов',
  en: ['visit', 'visits'],
} as const;

/** «47 забегов» — число и правильная форма слова. */
export function pluralize(count: number, forms: PluralForms): string {
  return `${count} ${plural(count, forms)}`;
}
