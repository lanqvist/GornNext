import { memo, useEffect, useRef, useState } from 'react';

import { tr } from '../core/i18n.js';
import { RUN, pluralize } from '../core/plural.js';
import type { DropView, RunView, SessionDropView, Snapshot } from '../main/engine.js';
import type { OverlayMode } from '../main/settings.js';
import { Fe, FeIcon } from './components/Fe.js';
import { ago, clock, NBSP, num } from './format.js';
import { Icon } from './overlay/icons.js';
import { useNow } from './useNow.js';

type Mode = OverlayMode;

/**
 * Где сейчас игрок. Оверлей перестраивает главную цифру под момент:
 * на карте важен доход этого забега, в городе — итог сессии, без игры —
 * только то, что Горн жив и ждёт.
 */
type Place = 'map' | 'town' | 'offline';

/** Оценка «ещё столько-то» для цели сессии — грубая, поэтому округлённая. */
const roughEta = (ms: number) => {
  const m = Math.max(1, Math.round(ms / 60_000));
  if (m < 60) return tr(`${m}${NBSP}м`, `${m}m`);
  const h = Math.floor(m / 60);
  const rest = m % 60;
  return rest > 0
    ? tr(`${h}${NBSP}ч ${rest}${NBSP}м`, `${h}h${NBSP}${rest}m`)
    : tr(`${h}${NBSP}ч`, `${h}h`);
};

/** Порог, до которого темп забега не показывается: знаменатель слишком мал. */
const WARMUP_MS = 60_000;

/** Лог молчит дольше этого — игра, скорее всего, закрыта (как в подсказке движка). */
const SILENT_MS = 120_000;

/** Строк находок по режимам: в компактном — только то, что решает. */
const DROP_ROWS: Record<Mode, number> = { minimal: 0, compact: 4, full: 8, strip: 0 };

function placeOf(snapshot: Snapshot | null, now: number): Place {
  if (!snapshot || snapshot.status === 'searching') return 'offline';
  if (snapshot.current) return 'map';
  if (snapshot.lastActivityAt === 0 || now - snapshot.lastActivityAt > SILENT_MS) return 'offline';
  return 'town';
}

const DEFAULT_PREFS: GornOverlayPrefs = {
  popups: true,
  sound: true,
  volume: 0.5,
  divineFe: 20,
  jackpotFe: 50,
  exaltedFe: 5,
  fontScale: 1,
  goalFe: 0,
};

export function OverlayApp() {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [mode, setMode] = useState<Mode>('compact');
  const [locked, setLocked] = useState(true);
  const [now, setNow] = useState(Date.now());
  const [prefs, setPrefs] = useState<GornOverlayPrefs>(DEFAULT_PREFS);
  // Автоуклонение: закреплённый оверлей получает mousemove даже с
  // пропуском кликов (forward: true) — при наведении панель гаснет
  // и не мешает целиться, а через полсекунды возвращается.
  const [dodge, setDodge] = useState(false);
  const dodgeTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const root = useRef<HTMLDivElement>(null);

  useEffect(() => {
    return () => {
      if (dodgeTimer.current) clearTimeout(dodgeTimer.current);
    };
  }, []);

  const onDodgeMove = () => {
    setDodge(true);
    if (dodgeTimer.current) clearTimeout(dodgeTimer.current);
    dodgeTimer.current = setTimeout(() => setDodge(false), 500);
  };

  // Окно подгоняется под содержимое: лишняя прозрачная площадь дорого
  // обходится композитору, а высота меняется по ходу забега — строк дропа
  // становится больше.
  useEffect(() => {
    const el = root.current;
    if (!el || !window.gorn) return;
    const observer = new ResizeObserver(() => {
      // Ширину main берёт только у ленты: у колонки она задана режимом.
      const r = el.getBoundingClientRect();
      window.gorn.overlay.reportHeight(r.height, r.width);
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    // В браузере main-процесса нет — показываем демо, чтобы вёрстку оверлея
    // можно было проверять без запуска игры. Подборы имитируются таймером:
    // без изменяющегося среза всплывашки и звук иначе не проверить.
    // Параметры адреса подменяют то, что в приложении приходит из main:
    // ?place=town|offline, ?mode=minimal|full, ?scale=1.25, ?goal=1000, ?locked=1.
    if (!window.gorn) {
      if (import.meta.env.DEV) {
        const params = new URLSearchParams(location.search);
        const place = params.get('place');
        const demoMode = params.get('mode');
        if (
          demoMode === 'minimal' ||
          demoMode === 'compact' ||
          demoMode === 'full' ||
          demoMode === 'strip'
        ) {
          setMode(demoMode);
        }
        setPrefs({
          ...DEFAULT_PREFS,
          fontScale: Number(params.get('scale')) || 1,
          goalFe: Number(params.get('goal')) || 0,
        });
        setLocked(params.get('locked') === '1');

        let timer: ReturnType<typeof setInterval> | undefined;
        void import('./demo.js').then((m) => {
          let current: Snapshot =
            place === 'town'
              ? { ...m.demoSnapshot, status: 'idle', current: null }
              : place === 'offline'
                ? {
                    ...m.demoSnapshot,
                    status: 'idle',
                    current: null,
                    lastActivityAt: 0,
                    hint: tr('Игра не запущена.', 'The game is not running.'),
                  }
                : m.demoSnapshot;
          setSnapshot(current);
          timer = setInterval(() => {
            const gained = Math.random() < 0.15 ? 55 + Math.random() * 40 : Math.random() * 6;
            const cur = current.current;
            if (!cur) return;
            current = {
              ...current,
              current: {
                ...cur,
                netFe: cur.netFe + gained,
                valueFe: cur.valueFe + gained,
                drops: cur.drops.map((d, i) =>
                  i === (gained > 50 ? 0 : 3)
                    ? {
                        ...d,
                        qty: d.qty + 1,
                        totalFe: (d.totalFe ?? 0) + gained,
                        unitFe: gained,
                      }
                    : d,
                ),
              },
            };
            setSnapshot(current);
          }, 2500);
        });
        return () => timer && clearInterval(timer);
      }
      return;
    }

    void window.gorn.getSnapshot().then((s) => s && setSnapshot(s));
    void window.gorn.overlay.getState().then((s) => {
      setMode(s.mode);
      setLocked(s.locked);
      setPrefs({
        popups: s.popups ?? true,
        sound: s.sound ?? true,
        volume: s.volume ?? 0.5,
        divineFe: s.divineFe ?? 20,
        jackpotFe: s.jackpotFe ?? 50,
        exaltedFe: s.exaltedFe ?? 5,
        fontScale: s.fontScale ?? 1,
        goalFe: s.goalFe ?? 0,
      });
    });
    const offSnapshot = window.gorn.onSnapshot(setSnapshot);
    const offMode = window.gorn.overlay.onMode((m) => setMode(m));
    const offLocked = window.gorn.overlay.onLocked(setLocked);
    const offPrefs = window.gorn.overlay.onPrefs(setPrefs);
    return () => {
      offSnapshot();
      offMode();
      offLocked();
      offPrefs();
    };
  }, []);

  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, []);

  const place = placeOf(snapshot, now);
  const run = snapshot?.current ?? null;
  const previous = snapshot?.recent[0] ?? null;
  const elapsed = run ? now - run.startedAt : 0;
  const median = snapshot?.rate.median ?? 0;
  const sessionNet = snapshot ? snapshot.totals.valueFe - snapshot.totals.costFe : 0;

  const pace = run && elapsed >= WARMUP_MS ? (run.netFe / elapsed) * 3_600_000 : null;
  // К норме — медиане последних 20 забегов, как в главном окне и таблице забегов:
  // медиана всего сезона копит слабые первые недели и завышала бы процент.
  const norm = snapshot?.norm ?? 0;
  const delta = pace !== null && norm > 0 ? Math.round(((pace - norm) / norm) * 100) : null;

  // «Тень» прошлого забега: сколько он имел к этой же секунде. Оценка
  // грубая — по его среднему темпу, дроп на деле сыплется рывками, —
  // поэтому подана с тильдой и как справка, а не как метрика.
  const ghost =
    run && previous && previous.durationMs != null && previous.durationMs > 0
      ? previous.netFe * Math.min(1, elapsed / previous.durationMs)
      : null;

  return (
    // Масштаб — через zoom: он растягивает и кегль, и отступы разом,
    // а окну main-процесс даёт пропорциональную ширину.
    <div
      ref={root}
      className={`ov ov-${mode} ov-${place} ${locked ? 'ov-locked' : 'ov-free'} ${
        locked && dodge ? 'ov-dodge' : ''
      }`}
      style={{ zoom: prefs.fontScale }}
      onMouseMove={locked ? onDodgeMove : undefined}
    >
      {/* Полоска для перетаскивания видна только в разблокированном режиме:
          в заблокированном клики всё равно уходят в игру. */}
      {!locked && (
        <div className="ov-grip">
          <span className="ov-brand">
            <FeIcon />
            {tr('Горн', 'Gorn')}
          </span>
          <div className="ov-actions">
            <button
              onClick={() => window.gorn?.overlay.setPrefs({ popups: !prefs.popups })}
              title={
                prefs.popups
                  ? tr('Выключить всплывашки', 'Turn off popups')
                  : tr('Включить всплывашки', 'Turn on popups')
              }
              className={prefs.popups ? '' : 'ov-off'}
            >
              <Icon name="popups" />
            </button>
            <button
              onClick={() => window.gorn?.overlay.setPrefs({ sound: !prefs.sound })}
              title={
                prefs.sound
                  ? tr('Выключить звук', 'Mute sound')
                  : tr('Включить звук', 'Unmute sound')
              }
              className={prefs.sound ? '' : 'ov-off'}
            >
              <Icon name={prefs.sound ? 'sound' : 'mute'} />
            </button>
            <button
              onClick={() => window.gorn?.overlay.cycleMode()}
              title={tr('Режим (Ctrl+Alt+M)', 'Mode (Ctrl+Alt+M)')}
            >
              <Icon name="mode" />
            </button>
            <button
              onClick={() => window.gorn?.overlay.toggleLock()}
              title={tr('Закрепить (Ctrl+Alt+L)', 'Lock (Ctrl+Alt+L)')}
            >
              <Icon name="lock" />
            </button>
            <button
              onClick={() => window.gorn?.overlay.close()}
              title={tr('Закрыть (Ctrl+Alt+G)', 'Close (Ctrl+Alt+G)')}
            >
              <Icon name="close" />
            </button>
          </div>
        </div>
      )}

      {/* Шпаргалка видна ровно тогда, когда оверлей настраивают. У ленты
          её нет: строка растянулась бы под текст. */}
      {!locked && mode !== 'strip' && (
        <div className="ov-hint">
          {tr(
            'Тяните за полоску. Закрепить — замок или Ctrl+Alt+L, режим — Ctrl+Alt+M.',
            'Drag by the strip. Lock with the padlock or Ctrl+Alt+L, mode — Ctrl+Alt+M.',
          )}
        </div>
      )}

      {mode === 'strip' && snapshot && (
        <StripLine
          place={place}
          run={run}
          elapsed={elapsed}
          delta={delta}
          sessionNet={sessionNet}
          previous={previous}
          snapshot={snapshot}
          prefs={prefs}
        />
      )}

      {mode !== 'strip' && (
      <>
      {/* Шапка: где игрок. На карте — таймер и название зоны. */}
      <div className="ov-head">
        {place === 'map' && run && (
          <>
            <span className="ov-chip">
              <Icon name="timer" />
              {clock(elapsed)}
            </span>
            <span className="ov-mapname">{run.name}</span>
          </>
        )}
        {place === 'town' && (
          <span className="ov-chip ov-chip-town">
            <Icon name="town" />
            {tr('в городе', 'in town')}
          </span>
        )}
        {place === 'offline' && (
          <span className="ov-chip ov-chip-off">
            <Icon name="offline" />
            {snapshot?.status === 'searching'
              ? tr('игра не найдена', 'game not found')
              : tr('нет игры', 'no game')}
          </span>
        )}
      </div>

      {/* Главная цифра: доход забега на карте, итог сессии в городе. */}
      {place === 'map' && run && (
        <div className="ov-hero">
          <Fe value={run.netFe} signed />
          {delta !== null && <Delta value={delta} />}
        </div>
      )}
      {place === 'town' && (
        <div className="ov-hero">
          <Fe value={sessionNet} signed />
          <span className="ov-hero-sub">{tr('за сессию', 'this session')}</span>
        </div>
      )}

      {/* Забег, час и сессия одним взглядом — на карте, кроме минимума. */}
      {place === 'map' && run && mode !== 'minimal' && snapshot && (
        <RunTrio pace={pace} liveRate={snapshot.liveRate} sessionNet={sessionNet} />
      )}

      <div className="ov-rows">
        {/* В компактном режиме на карте норма уже стоит за процентом у
            главной цифры, а строка ушла под тройку. */}
        {!(place === 'map' && mode === 'compact') && (
          <div className="ov-row">
            <Icon name="pace" />
            <span>{tr('темп, медиана', 'pace, median')}</span>
            <b>
              <Fe value={median} />
              <small>{tr('/ч', '/h')}</small>
            </b>
          </div>
        )}
        {mode !== 'minimal' && place === 'map' && ghost !== null && (
          <div
            className="ov-row ov-row-dim"
            title={tr(
              'Прошлый забег к этой же секунде — грубо, по его среднему темпу',
              'The previous run at this same second — rough, based on its average pace',
            )}
          >
            <Icon name="ghost" />
            <span>{tr('прошлый к этой секунде', 'last run by now')}</span>
            <b>~{num(ghost)}</b>
          </div>
        )}
        {mode !== 'minimal' && place === 'town' && previous && (
          <div className="ov-row ov-row-dim">
            <Icon name="map" />
            <span>
              {previous.name}
              {previous.durationMs != null && ` · ${clock(previous.durationMs)}`}
            </span>
            <b>
              <Fe value={previous.netFe} signed />
            </b>
          </div>
        )}
        {mode === 'full' && place !== 'offline' && snapshot && (
          <div className="ov-row ov-row-dim">
            <Icon name="runs" />
            <span>
              {pluralize(snapshot.totals.runs, RUN)} · {confidence(snapshot)}
            </span>
          </div>
        )}
        {mode !== 'minimal' && place === 'offline' && snapshot?.hint && (
          <div className="ov-note">{snapshot.hint}</div>
        )}
      </div>

      {place === 'map' && run && DROP_ROWS[mode] > 0 && snapshot && (
        <DropBars drops={run.drops} rows={DROP_ROWS[mode]} snapshot={snapshot} prefs={prefs} />
      )}

      {/* Дорогая находка не пропадает вместе с золотой плашкой. */}
      {mode !== 'minimal' && place !== 'offline' && snapshot && (
        <SessionFind top={snapshot.sessionTop[0] ?? null} goldFe={goldOf(prefs, snapshot)} />
      )}

      {/* Цель сессии: полоса прогресса с ETA по накопительному темпу.
          Работает и в минимальном режиме — ради него и затевалась. */}
      {prefs.goalFe > 0 && snapshot && place !== 'offline' && (
        <GoalProgress goalFe={prefs.goalFe} snapshot={snapshot} brief={mode === 'minimal'} />
      )}
      </>
      )}
    </div>
  );
}

function Delta({ value }: { value: number }) {
  return (
    <span
      className={`ov-delta ${value >= 0 ? 'up' : 'down'}`}
      title={tr(
        'Темп этой карты к норме — медиане последних 20 забегов',
        'This map’s pace vs the norm — the median of the last 20 runs',
      )}
    >
      <Icon name={value >= 0 ? 'up' : 'down'} />
      {Math.abs(value)}%
    </span>
  );
}

/**
 * «Лента» — оверлей в одну строку у края экрана, как HUD в гонках:
 * таймер и карта, доход забега с отклонением, темп и итог сессии, цель,
 * находки чипами по ступеням ценности и крупное за сессию. Подробности
 * не разворачиваются в самой ленте — их открывает пульт (Ctrl+Alt+T).
 */
function StripLine({
  place,
  run,
  elapsed,
  delta,
  sessionNet,
  previous,
  snapshot,
  prefs,
}: {
  place: Place;
  run: RunView | null;
  elapsed: number;
  delta: number | null;
  sessionNet: number;
  previous: RunView | null;
  snapshot: Snapshot;
  prefs: GornOverlayPrefs;
}) {
  const goldFe = goldOf(prefs, snapshot);
  const goal =
    prefs.goalFe > 0 && place !== 'offline' ? Math.max(0, Math.min(1, sessionNet / prefs.goalFe)) : null;

  return (
    <div className="ov-line">
      {place === 'map' && run && (
        <>
          <span className="ov-chip">
            <Icon name="timer" />
            {clock(elapsed)}
          </span>
          <span className="ov-mapname">{run.name}</span>
          <i className="ov-sep" />
          <span className="ov-line-hero">
            <Fe value={run.netFe} signed />
          </span>
          {delta !== null && <Delta value={delta} />}
        </>
      )}
      {place === 'town' && (
        <>
          <span className="ov-chip ov-chip-town">
            <Icon name="town" />
            {tr('в городе', 'in town')}
          </span>
          <i className="ov-sep" />
          <span className="ov-line-hero">
            <Fe value={sessionNet} signed />
          </span>
          <span className="ov-kv">{tr('за сессию', 'this session')}</span>
        </>
      )}
      {place === 'offline' && (
        <>
          <span className="ov-chip ov-chip-off">
            <Icon name="offline" />
            {snapshot.status === 'searching'
              ? tr('игра не найдена', 'game not found')
              : tr('нет игры', 'no game')}
          </span>
          <span className="ov-kv">
            {tr('медиана', 'median')} <b>{num(snapshot.rate.median)}</b>
            {tr('/ч', '/h')}
          </span>
        </>
      )}

      {place !== 'offline' && (
        <>
          <i className="ov-sep" />
          <span className="ov-kv" title={tr('Темп сессии вместе с этим забегом', 'Session pace including this run')}>
            {tr('в час', 'per hour')} <b>{num(snapshot.liveRate)}</b>
          </span>
          {place === 'map' && (
            <span className="ov-kv">
              {tr('сессия', 'session')}{' '}
              <b>
                {sessionNet > 0 ? '+' : ''}
                {num(sessionNet)}
              </b>
            </span>
          )}
          {place === 'town' && previous && (
            <span className="ov-kv">
              <span className="ov-mapname">{previous.name}</span>
              <b>
                {previous.netFe > 0 ? '+' : ''}
                {num(previous.netFe)}
              </b>
            </span>
          )}
          {goal !== null && (
            <span
              className={`ov-mgoal ${goal >= 1 ? 'ov-mgoal-done' : ''}`}
              title={tr(`Цель сессии: ${num(sessionNet)}/${num(prefs.goalFe)}`, `Session goal: ${num(sessionNet)}/${num(prefs.goalFe)}`)}
            >
              <i style={{ width: `${goal * 100}%` }} />
            </span>
          )}
        </>
      )}

      {place === 'map' && run && <StripDrops drops={run.drops} snapshot={snapshot} prefs={prefs} />}
      {place !== 'offline' && <StripFind top={snapshot.sessionTop[0] ?? null} goldFe={goldFe} />}
    </div>
  );
}

/** Находки забега чипами: иконка и цена, кромка — ступень ценности. Самые дорогие первыми. */
const StripDrops = memo(function StripDrops({
  drops,
  snapshot,
  prefs,
}: {
  drops: DropView[];
  snapshot: Snapshot;
  prefs: GornOverlayPrefs;
}) {
  const goldFe = goldOf(prefs, snapshot);
  const orangeFe = prefs.exaltedFe > 0 ? prefs.exaltedFe : goldFe / 5;
  const shown = drops
    .filter((d) => d.priceSource !== 'excluded' && d.totalFe !== null && d.totalFe >= snapshot.minDropFe)
    .slice(0, 4);
  if (shown.length === 0) return null;
  return (
    <>
      <i className="ov-sep" />
      <span className="ov-dchips">
        {shown.map((d) => {
          const fe = d.totalFe ?? 0;
          const tier = fe >= goldFe ? 'gold' : fe >= orangeFe ? 'orange' : 'plain';
          return (
            <span key={d.configBaseId} className={`ov-dchip t-${tier}`} title={`${d.name}${d.qty > 1 ? ` ×${num(d.qty)}` : ''}`}>
              <span className="ov-drop-icon">
                <ItemIcon drop={d} />
              </span>
              <Fe value={fe} />
            </span>
          );
        })}
      </span>
    </>
  );
});

/** Крупное за сессию в ленте — звезда с ценой, название во всплывающей подсказке. */
const StripFind = memo(function StripFind({
  top,
  goldFe,
}: {
  top: SessionDropView | null;
  goldFe: number;
}) {
  if (!top || goldFe <= 0 || top.totalFe < goldFe) return null;
  return (
    <span
      className="ov-dchip ov-sfind"
      title={tr(
        `Крупное за сессию: ${top.name}${top.qty > 1 ? ` ×${num(top.qty)}` : ''} · ${top.runName}`,
        `Top drop this session: ${top.name}${top.qty > 1 ? ` ×${num(top.qty)}` : ''} · ${top.runName}`,
      )}
    >
      <Icon name="star" />
      <span className="ov-drop-icon">
        <ItemIcon drop={top} />
      </span>
      <Fe value={top.totalFe} />
    </span>
  );
});


/** Порог «золота» тот же, что у плашек: явный из настроек либо авто (P95 цен). */
function goldOf(prefs: GornOverlayPrefs, snapshot: Snapshot): number {
  return prefs.jackpotFe > 0 ? prefs.jackpotFe : snapshot.jackpotAutoFe;
}

/**
 * Забег, час и сессия рядом: «иду ли я лучше обычного» и «сколько всего»
 * читаются одним взглядом, без режима «полный». Темп забега до минуты
 * не показывается — знаменатель слишком мал.
 */
function RunTrio({
  pace,
  liveRate,
  sessionNet,
}: {
  pace: number | null;
  liveRate: number;
  sessionNet: number;
}) {
  return (
    <div className="ov-trio">
      <div className="ov-trio-run" title={tr('Темп этого забега', 'This run’s pace')}>
        <small>{tr('забег, /ч', 'run, /h')}</small>
        <b>{pace === null ? '—' : num(pace)}</b>
      </div>
      <div title={tr('Темп сессии вместе с этим забегом', 'Session pace including this run')}>
        <small>{tr('в час', 'per hour')}</small>
        <b>{num(liveRate)}</b>
      </div>
      <div title={tr('Итог сессии', 'Session total')}>
        <small>{tr('сессия', 'session')}</small>
        <b>
          {sessionNet > 0 ? '+' : ''}
          {num(sessionNet)}
        </b>
      </div>
    </div>
  );
}

/**
 * Самая дорогая находка сессии — пока она «золотая». Плашка джекпота
 * живёт секунды; здесь находка стоит до конца сессии. Подсветка
 * статичная: постоянная анимация поверх игры стоит GPU.
 */
const SessionFind = memo(function SessionFind({
  top,
  goldFe,
}: {
  top: SessionDropView | null;
  goldFe: number;
}) {
  // Возраст раз в минуту: «14 мин назад» точнее и не нужно.
  const now = useNow(60_000);
  if (!top || goldFe <= 0 || top.totalFe < goldFe) return null;
  return (
    <div className="ov-find" title={tr('Крупное за сессию', 'Top drop this session')}>
      <Icon name="star" />
      <span className="ov-drop-icon">
        <ItemIcon drop={top} />
      </span>
      <span className="ov-find-text">
        <span className="ov-drop-name">
          {top.name}
          {top.qty > 1 && ` ×${num(top.qty)}`}
        </span>
        <small>
          {top.runName} · {ago(now - top.at)}
        </small>
      </span>
      <span className="ov-drop-fe">
        <Fe value={top.totalFe} />
      </span>
    </div>
  );
});

/**
 * Находки забега полосами, как в Details!: длина — доля от самой дорогой
 * находки, цвет кромки — ступень ценности (те же пороги, что у свиста
 * и золотой плашки). Мелочь и исключённое не показываются — как в списке
 * добычи главного окна.
 */
// memo: секундный тик часов перерисовывает оверлей, но не полосы дропа и цель —
// они меняются только с новым снимком.
const DropBars = memo(function DropBars({
  drops,
  rows,
  snapshot,
  prefs,
}: {
  drops: DropView[];
  rows: number;
  snapshot: Snapshot;
  prefs: GornOverlayPrefs;
}) {
  const goldFe = goldOf(prefs, snapshot);
  const orangeFe = prefs.exaltedFe > 0 ? prefs.exaltedFe : goldFe / 5;
  const shown = drops
    .filter(
      (d) =>
        d.priceSource !== 'excluded' && !(d.totalFe !== null && d.totalFe < snapshot.minDropFe),
    )
    .slice(0, rows);
  if (shown.length === 0) return null;
  const max = Math.max(1, ...shown.map((d) => d.totalFe ?? 0));

  const tier = (fe: number | null) =>
    fe === null ? 'none' : fe >= goldFe ? 'gold' : fe >= orangeFe ? 'orange' : 'plain';

  return (
    <ul className="ov-drops">
      {shown.map((d) => (
        <li key={d.configBaseId} className={`t-${tier(d.totalFe)}`}>
          <i style={{ width: `${d.totalFe === null ? 0 : (d.totalFe / max) * 100}%` }} />
          <span className="ov-drop-icon">
            <ItemIcon drop={d} />
          </span>
          <span className="ov-drop-name">{d.name}</span>
          {d.qty > 1 && <span className="ov-drop-qty">×{num(d.qty)}</span>}
          <span className="ov-drop-fe">
            {d.totalFe === null ? '—' : <Fe value={d.totalFe} />}
          </span>
        </li>
      ))}
    </ul>
  );
});

/** Иконка предмета; без неё или при ошибке загрузки — первая буква на плашке. */
function ItemIcon({ drop }: { drop: { icon: string | null; name: string } }) {
  const [failed, setFailed] = useState(false);
  if (!drop.icon || failed) return <span className="ov-drop-ph">{drop.name.slice(0, 1)}</span>;
  return <img src={drop.icon} alt="" draggable={false} onError={() => setFailed(true)} />;
}

const GoalProgress = memo(function GoalProgress({
  goalFe,
  snapshot,
  brief,
}: {
  goalFe: number;
  snapshot: Snapshot;
  brief: boolean;
}) {
  const net = snapshot.totals.valueFe - snapshot.totals.costFe;
  const share = Math.max(0, Math.min(1, net / goalFe));
  const remaining = goalFe - net;
  const etaMs =
    remaining > 0 && snapshot.liveRate > 0 ? (remaining / snapshot.liveRate) * 3_600_000 : null;

  return (
    <div
      className={`ov-goal ${remaining <= 0 ? 'ov-goal-done' : ''}`}
      title={tr('Цель сессии', 'Session goal')}
    >
      <Icon name={remaining <= 0 ? 'check' : 'goal'} />
      <div className="ov-goal-bar">
        <i style={{ width: `${share * 100}%` }} />
      </div>
      <span className="ov-goal-text">
        {remaining <= 0
          ? tr('цель взята', 'goal reached')
          : `${num(net)}/${num(goalFe)}${
              etaMs !== null && !brief
                ? tr(` · ещё ~${roughEta(etaMs)}`, ` · ~${roughEta(etaMs)} left`)
                : ''
            }`}
      </span>
    </div>
  );
});

function confidence(snapshot: Snapshot): string {
  switch (snapshot.rate.confidence) {
    case 'insufficient':
      return tr('данных мало', 'not enough data');
    case 'emerging':
      return tr('есть картина', 'taking shape');
    default:
      return tr('надёжно', 'reliable');
  }
}
