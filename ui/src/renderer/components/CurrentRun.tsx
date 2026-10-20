import { useState } from 'react';

import { tr } from '../../core/i18n.js';
import type { RunView, Snapshot } from '../../main/engine.js';
import { clock, duration, num } from '../format.js';
import { DropList } from './DropList.js';
import { Fe } from './Fe.js';
import { useNow } from '../useNow.js';

/**
 * Сколько карта должна идти, прежде чем показывать сравнение темпа
 * с обычным. Меньше минуты знаменатель слишком мал: одна находка
 * даёт скачок в сотни процентов.
 */
const WARMUP_MS = 60_000;

/** Часы забега: тикают сами, остальная карточка (дроп) от таймера не перерисовывается. */
function RunTimer({ startedAt }: { startedAt: number }) {
  const now = useNow(1000);
  return <span className="timer">{clock(now - startedAt)}</span>;
}

/** Темп к обычному: зависит от времени, поэтому тоже тикает сам. */
function RunPace({ startedAt, netFe, median }: { startedAt: number; netFe: number; median: number }) {
  const now = useNow(1000);
  if (median <= 0) return null;
  const elapsed = now - startedAt;
  // Пока карта только началась, темп считается по крошечному знаменателю
  // и скачет на сотни процентов от одной находки. Показывать такое число —
  // значит врать: сравнение появляется только после разогрева.
  if (elapsed < WARMUP_MS) {
    return <span className="delta muted">{tr('темп считается…', 'calculating pace…')}</span>;
  }
  const pace = (netFe / elapsed) * 3_600_000;
  const delta = pace - median;
  const pct = Math.round((delta / median) * 100);
  return (
    <span className={`delta ${delta >= 0 ? 'up' : 'down'}`}>
      {delta >= 0 ? '↑' : '↓'}{' '}
      {tr(`${Math.abs(pct)}% к обычному темпу`, `${Math.abs(pct)}% vs usual pace`)}
    </span>
  );
}

export function CurrentRun({ snapshot }: { snapshot: Snapshot }) {
  const run = snapshot.current;

  // Карта закончилась — показываем прошлый забег, а не пустую карточку.
  // Иначе добыча исчезает с экрана ровно в тот момент, когда её хотят
  // рассмотреть: сразу после выхода из карты.
  if (!run) {
    const previous = snapshot.recent[0];
    if (!previous) {
      // Совсем без данных — это первый запуск: человеку нужны шаги,
      // а не констатация «ждём карту».
      if (snapshot.status === 'searching') return <GameNotFound />;
      return <Onboarding snapshot={snapshot} />;
    }
    return <FinishedRun run={previous} minFe={snapshot.minDropFe} />;
  }

  return (
    <section className="card current">
      <div className="current-head">
        <h2>{run.name}</h2>
        <RunTimer startedAt={run.startedAt} />
      </div>

      <div className="current-value">
        <Fe value={run.netFe} />
        <RunPace startedAt={run.startedAt} netFe={run.netFe} median={snapshot.rate.median} />
      </div>

      <DropList drops={run.drops} minFe={snapshot.minDropFe} />
    </section>
  );
}

/**
 * «Игра не найдена» — раньше это был тупик без единого действия.
 * Автопоиск повторяется сам, а кнопка спасает не-Steam установки.
 */
function GameNotFound() {
  const [note, setNote] = useState<string | null>(null);
  return (
    <section className="card current empty onboarding">
      <h2>{tr('Игра не найдена', 'Game not found')}</h2>
      <p className="muted">
        {tr(
          'Steam-установка находится сама, поиск повторяется автоматически. Если игра стоит в другом месте или у вас не-Steam версия — укажите её папку.',
          'A Steam install is found automatically, and the search keeps retrying. If the game is installed elsewhere or you have a non-Steam version, point to its folder.',
        )}
      </p>
      <div className="set-actions">
        <button
          className="set-btn"
          onClick={() => {
            void window.gorn?.settings.chooseLogDir().then((r) => {
              if (r.status === 'invalid') {
                setNote(
                  tr(
                    'В выбранной папке нет логов игры — нужен корень установки или …\\Saved\\Logs.',
                    'No game logs in that folder — pick the install root or …\\Saved\\Logs.',
                  ),
                );
              } else {
                setNote(null);
              }
            });
          }}
        >
          {tr('Указать папку с игрой…', 'Choose game folder…')}
        </button>
      </div>
      {note && <p className="note warn">{note}</p>}
    </section>
  );
}

/** Первый запуск без истории: три шага вместо пустого «ждём карту». */
function Onboarding({ snapshot }: { snapshot: Snapshot }) {
  return (
    <section className="card current empty onboarding">
      <h2>{tr('Всё готово к первому забегу', 'Ready for your first run')}</h2>
      <ol className="steps">
        <li>{tr('Запустите Torchlight: Infinite', 'Launch Torchlight: Infinite')}</li>
        <li>{tr('Зайдите на любую карту', 'Enter any map')}</li>
        <li>
          {tr(
            'Дальше я сам: посчитаю добычу, вложения и честный темп',
            'I’ll take it from there: loot, map costs and your honest pace',
          )}
        </li>
      </ol>
      {snapshot.hint && <p className="muted small">{snapshot.hint}</p>}
      {snapshot.logDir && (
        <p className="muted small set-path">
          {tr('Лог найден:', 'Log found:')} {snapshot.logDir}
        </p>
      )}
      <p className="muted small">
        {tr(
          'Оверлей поверх игры — кнопка в шапке или Ctrl+Alt+G. Игре нужен безрамочный или оконный полноэкранный режим: эксклюзивный полный экран перекрыть нельзя.',
          'Overlay on top of the game: the button in the header or Ctrl+Alt+G. The game needs borderless or windowed fullscreen — exclusive fullscreen can’t be drawn over.',
        )}
      </p>
    </section>
  );
}

/** Последний завершённый забег: держим его на экране, пока не начнётся новый. */
function FinishedRun({ run, minFe }: { run: RunView; minFe: number }) {
  return (
    <section className="card current">
      <div className="current-head">
        <h2>
          {run.name} <span className="muted small">· {tr('прошлый забег', 'last run')}</span>
        </h2>
        <span className="timer">{duration(run.durationMs ?? 0)}</span>
      </div>

      <div className="current-value">
        <Fe value={run.netFe} />
        <span className="delta muted">
          {tr(`${num(run.perHour)} в час`, `${num(run.perHour)}/h`)}
        </span>
      </div>

      <DropList drops={run.drops} minFe={minFe} />
    </section>
  );
}
