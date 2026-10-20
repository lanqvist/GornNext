/**
 * Язык интерфейса.
 *
 * Без библиотеки и без ключей: перевод стоит рядом с оригиналом —
 * `tr('Забеги', 'Runs')`. Строк несколько сотен, и ключи вида
 * `runs.table.header.duration` только отрывали бы текст от места,
 * где его читают и правят.
 *
 * Состояние — на модуль: у main-процесса и у каждого окна свой экземпляр.
 * Окна при смене языка перезагружаются, main переключается на лету.
 */

export type UiLanguage = 'ru' | 'en';

let current: UiLanguage = 'ru';

export function setUiLanguage(language: UiLanguage): void {
  current = language;
}

export function uiLanguage(): UiLanguage {
  return current;
}

/** Строка на текущем языке. */
export function tr(ru: string, en: string): string {
  return current === 'en' ? en : ru;
}

/** Локаль для Intl: числа, даты, порядок «12 окт» / «Oct 12». */
export function uiLocale(): string {
  return current === 'en' ? 'en-US' : 'ru-RU';
}

/**
 * Язык новой установки по системной локали: русская — русский, любая
 * другая — английский. За носителей других языков не решаем, какой
 * из двух им ближе: переключатель в настройках есть у всех.
 */
export function languageForLocale(locale: string | null | undefined): UiLanguage {
  return /^ru\b/i.test(locale ?? '') ? 'ru' : 'en';
}

export function isUiLanguage(value: unknown): value is UiLanguage {
  return value === 'ru' || value === 'en';
}
