import { tr } from './i18n.js';

/**
 * Названия сезонов по SeasonId из профиля игрока.
 *
 * Номер растёт на сотню за сезон (1501 — SS13, 1601 — SS14), поэтому
 * неизвестный ещё сезон получает хотя бы номер «SSN», а имя дописывается
 * сюда, когда сезон вышел. Имена — собственные, как в игре и у TITrack,
 * без перевода.
 */
const SEASONS = new Map<number, string>([
  [1301, 'SS11 Vorax'],
  [1401, 'SS12 Lunaria'],
  [1501, 'SS13 Afterlight'],
  [1601, 'SS14 Tartarus'],
]);

/** Постоянный (несезонный) сервер. */
const PERMANENT_SEASON = 1;

export function seasonName(seasonId: number | null): string | null {
  if (seasonId === null) return null;
  if (seasonId === PERMANENT_SEASON) return tr('Постоянный сервер', 'Permanent server');
  const known = SEASONS.get(seasonId);
  if (known) return known;
  if (seasonId > 1000 && seasonId % 100 === 1) return `SS${Math.floor(seasonId / 100) - 2}`;
  return tr(`Сезон ${seasonId}`, `Season ${seasonId}`);
}

/** Короткая метка для списков: «SS14». */
export function seasonShort(seasonId: number | null): string | null {
  const name = seasonName(seasonId);
  return name?.match(/^SS\d+/)?.[0] ?? name;
}
