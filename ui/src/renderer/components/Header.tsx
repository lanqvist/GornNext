import { useEffect, useState } from 'react';

import { tr } from '../../core/i18n.js';
import { RUN, pluralize } from '../../core/plural.js';
import { seasonShort } from '../../core/seasons.js';
import type { Snapshot } from '../../main/engine.js';
import { ago } from '../format.js';
import { useNow } from '../useNow.js';
import { FeIcon } from './Fe.js';
import { Tip } from './ui.js';

/**
 * Статус трекера: точка, «на карте / ожидание», возраст лога. Тикает сам
 * (useNow) — остальная шапка от секундного таймера не перерисовывается.
 */
function LogStatus({ snapshot }: { snapshot: Snapshot }) {
  const now = useNow(1000);
  const dot =
    snapshot.status === 'running' ? 'live' : snapshot.status === 'idle' ? 'idle' : 'lost';

  // «Трекер жив»: возраст последнего распознанного события лога.
  // Считается на клиенте от тикающего now — снапшоту не нужно тикать самому.
  const age = snapshot.lastActivityAt > 0 ? Math.max(0, now - snapshot.lastActivityAt) : null;
  const showAge = age !== null && (snapshot.status === 'running' || age < 600_000);
  const ageTone =
    age === null ? '' : age < 15_000 ? 'fresh' : age < 60_000 ? 'ok' : 'stale';

  return (
    <div
      className={`status status-${dot} ${age !== null && age < 3_000 ? 'status-pulse' : ''}`}
    >
      <i />
      {snapshot.status === 'running'
        ? tr('На карте', 'On a map')
        : snapshot.status === 'idle'
          ? tr('Ожидание', 'Waiting')
          : tr('Игра не найдена', 'Game not found')}
      {showAge && (
        <span className={`log-age log-age-${ageTone}`}>
          {tr('лог', 'log')} {ago(age)}
        </span>
      )}
      {snapshot.hint && <span className="hint">{snapshot.hint}</span>}
    </div>
  );
}

export function Header({
  snapshot,
  onOpenSettings,
}: {
  snapshot: Snapshot;
  onOpenSettings: () => void;
}) {
  return (
    <header className="header">
      <div className="brand">
        <FeIcon />
        <span>{tr('Горн', 'Gorn')}</span>
      </div>
      <LogStatus snapshot={snapshot} />
      <PlayerChip player={snapshot.player} />
      <PlayerFilter snapshot={snapshot} />
      <div className="header-right">
        <OverlayButton />
        <button
          className="overlay-toggle"
          title={tr('Настройки', 'Settings')}
          onClick={onOpenSettings}
        >
          ⚙ {tr('Настройки', 'Settings')}
        </button>
        {snapshot.seasonId !== null && (
          <span
            className="season"
            title={tr(
              `Сезон из профиля персонажа в логе (SeasonId ${snapshot.seasonId}). Цены и цифры — этого сезона.`,
              `Season from the character profile in the log (SeasonId ${snapshot.seasonId}). Prices and numbers are for this season.`,
            )}
          >
            {snapshot.seasonName ?? tr(`Сезон ${snapshot.seasonId}`, `Season ${snapshot.seasonId}`)}
          </span>
        )}
      </div>
    </header>
  );
}

/**
 * Кем зашли в игру: ник, класс и уровень.
 *
 * После перезапуска игры персонаж неизвестен до следующего входа —
 * тогда чип просто не показывается, а не врёт прошлым значением.
 */
function PlayerChip({ player }: { player: Snapshot['player'] }) {
  if (!player) return null;
  const parts = [
    player.heroName,
    player.level !== null ? tr(`${player.level} ур.`, `lvl ${player.level}`) : null,
  ].filter(Boolean);
  return (
    <Tip
      tip={tr(
        'Персонаж, которым зашли в игру, — из лога. Новые забеги записываются на него.',
        'The character you logged in with, read from the log. New runs are recorded to it.',
      )}
    >
      <span className="player-chip">
        <span className="player-chip-name">{player.name ?? tr('Персонаж', 'Character')}</span>
        {parts.length > 0 && <span className="player-chip-meta">{parts.join(' · ')}</span>}
      </span>
    </Tip>
  );
}

/**
 * Чьи забеги показывать: текущий сезон, все сезоны или один персонаж.
 *
 * С началом сезона экономика обнуляется, и медиана прошлого сезона
 * описывает другую игру — поэтому по умолчанию виден только текущий.
 * У разных героев разные билды и дроп, смешивать их медианы — то самое
 * «среднее по больнице». Выпадашка появляется, только когда есть что
 * выбирать: второй персонаж или забеги других сезонов.
 */
function PlayerFilter({ snapshot }: { snapshot: Snapshot }) {
  const { players, playerFilter, allSeasons, otherSeasonRuns, seasonId } = snapshot;
  if (players.length < 2 && otherSeasonRuns === 0 && playerFilter === null && !allSeasons) {
    return null;
  }

  // У персонажа из оборванного лога может не быть профиля — только id.
  // Голое 18-значное число в выпадашке нечитаемо, показываем хвост.
  // Сезон подписывается только у прошлых персонажей: текущие — по умолчанию.
  const label = (p: Snapshot['players'][number]) =>
    [
      p.heroName ??
        p.name ??
        tr(`Персонаж …${p.playerId.slice(-5)}`, `Character …${p.playerId.slice(-5)}`),
      p.level !== null ? tr(`${p.level} ур.`, `lvl ${p.level}`) : null,
      p.seasonId !== null && p.seasonId !== seasonId ? seasonShort(p.seasonId) : null,
    ]
      .filter(Boolean)
      .join(' · ');

  const current = seasonShort(seasonId);
  const value = playerFilter !== null ? `p:${playerFilter}` : allSeasons ? 'all' : 'season';

  return (
    <select
      className="player-filter"
      value={value}
      title={tr(
        'Чьи забеги показывать. Влияет на все цифры; Торговый дом — в границах выбранного сезона. Забеги прошлых сезонов — в «Все сезоны».',
        'Whose runs to show. Affects every number; the Exchange is limited to the chosen season. Runs from past seasons are under “All seasons”.',
      )}
      onChange={(e) => {
        const v = e.target.value;
        void window.gorn?.setRunScope(
          v.startsWith('p:')
            ? { playerId: v.slice(2), allSeasons: false }
            : { playerId: null, allSeasons: v === 'all' },
        );
      }}
    >
      <option value="season">
        {current
          ? tr(`${current} · все персонажи`, `${current} · all characters`)
          : tr('Все персонажи', 'All characters')}
      </option>
      {(otherSeasonRuns > 0 || allSeasons) && (
        <option value="all">{tr('Все сезоны', 'All seasons')}</option>
      )}
      {players.map((p) => (
        <option key={p.playerId} value={`p:${p.playerId}`}>
          {label(p)} — {pluralize(p.runs, RUN)}
        </option>
      ))}
    </select>
  );
}

function OverlayButton() {
  const [open, setOpen] = useState(false);

  useEffect(() => {
    if (!window.gorn) return;
    void window.gorn.overlay.getState().then((s) => setOpen(s.enabled));
    // Оверлей переключается и хоткеем, и собственным крестиком — кнопка
    // обязана узнавать об этом, а не помнить только свой последний клик.
    return window.gorn.overlay.onEnabled(setOpen);
  }, []);

  if (!window.gorn) return null;

  return (
    <button
      className={`overlay-toggle ${open ? 'on' : ''}`}
      onClick={() => void window.gorn.overlay.toggle().then(setOpen)}
      title={tr(
        'Ctrl+Alt+G — показать или убрать оверлей поверх игры',
        'Ctrl+Alt+G — show or hide the overlay on top of the game',
      )}
    >
      {open ? tr('Оверлей включён', 'Overlay on') : tr('Оверлей', 'Overlay')}
    </button>
  );
}
