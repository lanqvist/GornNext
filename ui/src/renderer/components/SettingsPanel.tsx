import { useEffect, useState, type ReactNode } from 'react';

import type { UpdateState } from '../../main/updater.js';
import { tr, uiLanguage, type UiLanguage } from '../../core/i18n.js';
import { num } from '../format.js';
import { playPreview, playTest, type SoundPreview } from '../overlay/sound.js';

/**
 * Кнопки прослушивания: каждый звук как в игре — громкость и режим
 * подбираются на слух, без ожидания золотой находки на карте.
 */
const SOUND_PREVIEWS: ReadonlyArray<readonly [SoundPreview, string, string]> = [
  ['exalted', 'Возвышение', 'Exalted'],
  ['divine', 'Дивайн', 'Divine'],
  ['record', 'Рекорд', 'Record'],
];

/** Пороги звуков находок: от «почти всё ценное» до «только дорогое». */
const EXALTED_STEPS: readonly number[] = [3, 5, 10, 20];
const DIVINE_STEPS: readonly number[] = [10, 20, 50, 100];

/**
 * Язык названий предметов.
 *
 * Русские покрывают 96% справочника, но часть игроков привыкла к английским
 * — по ним ищут на бирже и в гайдах. Режим «оба» показывает и то и другое.
 */
function NameLanguageSwitch() {
  const [language, setLanguage] = useState<NameLanguage>('ru');

  useEffect(() => {
    if (!window.gorn) return;
    void window.gorn.getItemNames().then(setLanguage);
  }, []);

  const options: Array<[NameLanguage, string, string]> = [
    ['ru', tr('Рус', 'Rus'), tr('Русские названия предметов', 'Russian item names')],
    [
      'en',
      'Eng',
      tr('Английские названия — как в гайдах и на бирже', 'English names, as in guides and on the Exchange'),
    ],
    ['both', tr('Оба', 'Both'), tr('Русское и английское рядом', 'Russian and English side by side')],
  ];

  return (
    <div
      className="lang-switch"
      role="group"
      aria-label={tr('Язык названий предметов', 'Item name language')}
    >
      {options.map(([value, label, hint]) => (
        <button
          key={value}
          className={language === value ? 'on' : ''}
          title={hint}
          onClick={() => {
            if (!window.gorn) return setLanguage(value);
            void window.gorn.setItemNames(value).then(setLanguage);
          }}
        >
          {label}
        </button>
      ))}
    </div>
  );
}

/**
 * Язык интерфейса. Каждый вариант назван на самом себе — игрок должен
 * найти свой язык, даже если интерфейс сейчас на чужом. Окно после смены
 * перезагружается само, поэтому своё состояние кнопкам не нужно.
 */
function UiLanguageSwitch() {
  const current = uiLanguage();
  const options: Array<[UiLanguage, string]> = [
    ['ru', 'Русский'],
    ['en', 'English'],
  ];

  return (
    <div
      className="lang-switch"
      role="group"
      aria-label={tr('Язык интерфейса', 'Interface language')}
    >
      {options.map(([value, label]) => (
        <button
          key={value}
          className={current === value ? 'on' : ''}
          onClick={() => void window.gorn?.setUiLanguage(value)}
        >
          {label}
        </button>
      ))}
    </div>
  );
}

/**
 * Панель настроек — единственное место, где собраны все ручки.
 *
 * До неё настройки были размазаны: язык в шапке, цены сообщества
 * в самодиагностике, звук и плашки — на полоске откреплённого оверлея,
 * а громкость и прозрачность не имели контролов вовсе. Игрок, который
 * хочет «сделать потише», должен находить это за один клик.
 */
export function SettingsPanel({
  onClose,
  minDropFe,
  children,
}: {
  onClose: () => void;
  /** Порог «мелочи» живёт в срезе: меняется кнопкой и сразу видна в списках. */
  minDropFe: number;
  /** Низ панели — самодиагностика: она для тех, кто настраивает. */
  children?: ReactNode;
}) {
  const [prefs, setPrefs] = useState({
    popups: true,
    sound: true,
    volume: 0.5,
    divineFe: 20,
    jackpotFe: 50,
    exaltedFe: 5,
    fontScale: 1,
    opacity: 0.9,
    goalFe: 0,
  });
  const [sys, setSys] = useState<GornAppSettings | null>(null);
  const [community, setCommunity] = useState(true);
  const [prisonHints, setPrisonHints] = useState(false);
  const [prisonShots, setPrisonShots] = useState(false);
  const [logDirNote, setLogDirNote] = useState<string | null>(null);
  const [needsRestart, setNeedsRestart] = useState(false);
  const [updState, setUpdState] = useState<UpdateState | null>(null);
  const [updChecked, setUpdChecked] = useState(false);

  const refreshSys = () => {
    void window.gorn?.settings.get().then(setSys);
  };

  useEffect(() => {
    if (!window.gorn) return;
    void window.gorn.overlay.getState().then((s) => {
      setPrefs({
        popups: s.popups ?? true,
        sound: s.sound ?? true,
        volume: s.volume ?? 0.5,
        divineFe: s.divineFe ?? 20,
        jackpotFe: s.jackpotFe ?? 50,
        exaltedFe: s.exaltedFe ?? 5,
        fontScale: s.fontScale ?? 1,
        opacity: s.opacity ?? 0.9,
        goalFe: s.goalFe ?? 0,
      });
    });
    refreshSys();
    void window.gorn.getCommunityPrices().then(setCommunity);
    void window.gorn.getPrisonHints().then(setPrisonHints);
    void window.gorn.getPrisonSaveShots().then(setPrisonShots);
    return window.gorn.update.onState(setUpdState);
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  /** Прозрачность идёт своим каналом, остальное — общим patchPrefs. */
  const patch = (p: Partial<typeof prefs>) => {
    setPrefs((s) => ({ ...s, ...p }));
    const { opacity, ...rest } = p;
    if (Object.keys(rest).length > 0) window.gorn?.overlay.setPrefs(rest);
    if (opacity !== undefined) window.gorn?.overlay.setOpacity(opacity);
  };


  const chooseDir = () => {
    void window.gorn?.settings.chooseLogDir().then((r) => {
      if (r.status === 'invalid') {
        setLogDirNote(
          tr(
            'В выбранной папке нет логов игры — нужна …\\Saved\\Logs или корень установки.',
            'No game logs in this folder. Pick …\\Saved\\Logs or the game install folder.',
          ),
        );
        return;
      }
      setLogDirNote(null);
      if (r.status === 'needsRestart') setNeedsRestart(true);
      if (r.status !== 'cancelled') refreshSys();
    });
  };

  const resetDir = () => {
    void window.gorn?.settings.resetLogDir().then((r) => {
      setLogDirNote(null);
      if (r.status === 'needsRestart') setNeedsRestart(true);
      refreshSys();
    });
  };

  const updateNote = !updChecked
    ? null
    : updState === null || updState.status === 'checking'
      ? tr('проверяю…', 'checking…')
      : updState.status === 'idle'
        ? tr('у вас последняя версия', 'you have the latest version')
        : updState.status === 'error'
          ? tr(
              `не получилось: ${updState.error ?? 'ошибка'}`,
              `check failed: ${updState.error ?? 'error'}`,
            )
          : tr('вышла новая версия — смотрите полосу сверху', 'new version out — see the bar at the top');

  return (
    <>
      <div className="settings-backdrop" onClick={onClose} />
      <aside className="settings-panel" role="dialog" aria-label={tr('Настройки', 'Settings')}>
        <div className="settings-head">
          <h2>{tr('Настройки', 'Settings')}</h2>
          <button className="update-close" title={tr('Закрыть', 'Close')} onClick={onClose}>
            ✕
          </button>
        </div>

        <section className="set-section">
          <h3>{tr('Звук', 'Sound')}</h3>
          <div className="lang-switch set-seg" role="group" aria-label={tr('Звук', 'Sound')}>
            <button
              className={prefs.sound ? 'on' : ''}
              title={tr(
                'Звучат только дорогие находки и рекорд; обычные подборы и итог карты молчат',
                'Only pricey drops and a record play a sound; regular pickups and the map total stay silent',
              )}
              onClick={() => patch({ sound: true })}
            >
              {tr('Звук', 'Sound on')}
            </button>
            <button className={prefs.sound ? '' : 'on'} onClick={() => patch({ sound: false })}>
              {tr('Без звука', 'Muted')}
            </button>
          </div>
          <label className={`set-row ${prefs.sound ? '' : 'set-disabled'}`}>
            <span>{tr('Громкость', 'Volume')}</span>
            <input
              type="range"
              min={0}
              max={100}
              value={Math.round(prefs.volume * 100)}
              disabled={!prefs.sound}
              onChange={(e) => patch({ volume: Number(e.target.value) / 100 })}
              onPointerUp={() => playTest(prefs.volume)}
            />
          </label>
          <ThresholdRow
            label={tr('«Возвышение» на предмет дороже', '“Exalted” for items over')}
            hint={tr(
              'Как в фильтрах PoE: ценный предмет слышно не глядя',
              'Like PoE loot filters: you hear a valuable item without looking',
            )}
            value={prefs.exaltedFe}
            steps={EXALTED_STEPS}
            disabled={!prefs.sound}
            onChange={(exaltedFe) => {
              patch({ exaltedFe });
              if (exaltedFe > 0) playPreview('exalted', prefs.volume);
            }}
          />
          <ThresholdRow
            label={tr('«Дивайн» на предмет дороже', '“Divine” for items over')}
            hint={tr(
              'Звук дорогой находки; золотая находка звучит им же',
              'The pricey-drop sound; golden drops use it too',
            )}
            value={prefs.divineFe}
            steps={DIVINE_STEPS}
            disabled={!prefs.sound}
            onChange={(divineFe) => {
              patch({ divineFe });
              if (divineFe > 0) playPreview('divine', prefs.volume);
            }}
          />
          <div className={`set-dir ${prefs.sound ? '' : 'set-disabled'}`}>
            <span className="muted small">{tr('Прослушать', 'Preview')}</span>
            <div className="set-actions">
              {SOUND_PREVIEWS.map(([kind, ru, en]) => (
                <button
                  key={kind}
                  className="set-btn"
                  disabled={!prefs.sound}
                  onClick={() => playPreview(kind, prefs.volume)}
                >
                  {tr(ru, en)}
                </button>
              ))}
            </div>
          </div>
        </section>

        <section className="set-section">
          <h3>{tr('Плашки поверх игры', 'In-game popups')}</h3>
          <label className="set-row">
            <span>{tr('Всплывающий доход у центра экрана', 'Income popups near screen center')}</span>
            <input
              type="checkbox"
              checked={prefs.popups}
              onChange={(e) => patch({ popups: e.target.checked })}
            />
          </label>
          <label className="set-row">
            <span>{tr('Золотая плашка', 'Golden popup')}</span>
            <select
              value={String(prefs.jackpotFe)}
              onChange={(e) => patch({ jackpotFe: Number(e.target.value) })}
            >
              <option value="0">
                {tr('авто — дорогое для сезона', 'auto — pricey for the season')}
              </option>
              <option value="10">{tr('от 10 ФЕ — почти на всё', '10+ FE — almost everything')}</option>
              <option value="50">{tr('от 50 ФЕ', '50+ FE')}</option>
              <option value="200">{tr('от 200 ФЕ — только дорогое', '200+ FE — pricey only')}</option>
              {![0, 10, 50, 200].includes(prefs.jackpotFe) && (
                <option value={String(prefs.jackpotFe)}>
                  {tr(`от ${prefs.jackpotFe} ФЕ`, `${prefs.jackpotFe}+ FE`)}
                </option>
              )}
            </select>
          </label>
        </section>

        <section className="set-section">
          <h3>{tr('Оверлей', 'Overlay')}</h3>
          <label className="set-row">
            <span>{tr('Прозрачность', 'Opacity')}</span>
            <input
              type="range"
              min={25}
              max={100}
              value={Math.round(prefs.opacity * 100)}
              onChange={(e) => patch({ opacity: Number(e.target.value) / 100 })}
            />
          </label>
          <label className="set-row">
            <span>{tr('Масштаб', 'Scale')}</span>
            <input
              type="range"
              min={80}
              max={160}
              step={10}
              value={Math.round(prefs.fontScale * 100)}
              onChange={(e) => patch({ fontScale: Number(e.target.value) / 100 })}
            />
          </label>
          <label className="set-row">
            <span
              title={tr(
                'Оверлей покажет полосу прогресса и оценку, сколько ещё фармить при текущем темпе. 0 — выключено.',
                'The overlay shows a progress bar and how much longer to farm at the current pace. 0 turns it off.',
              )}
            >
              {tr('Цель сессии, ФЕ', 'Session goal, FE')}
            </span>
            <input
              type="number"
              min={0}
              step={100}
              className="set-num"
              value={prefs.goalFe}
              onChange={(e) => patch({ goalFe: Math.max(0, Number(e.target.value) || 0) })}
            />
          </label>
          <p className="muted small">
            {tr(
              'Ctrl+Alt+G — показать · Ctrl+Alt+L — закрепить · Ctrl+Alt+M — режим · Ctrl+Alt+T — пульт · Ctrl+Alt+F — цены',
              'Ctrl+Alt+G — show · Ctrl+Alt+L — pin · Ctrl+Alt+M — mode · Ctrl+Alt+T — panel · Ctrl+Alt+F — prices',
            )}
          </p>
          {sys && sys.busyHotkeys.length > 0 && (
            <p className="note warn">
              {sys.busyHotkeys.join(', ')}{' '}
              {tr(
                '— занято другой программой, сочетание не сработает. Управляйте оверлеем кнопками из этого окна.',
                '— taken by another program, so the shortcut won’t work. Control the overlay with the buttons in this window.',
              )}
            </p>
          )}
          <p className="muted small">
            {tr(
              'Не видно оверлея в игре? Переключите её в безрамочный режим — эксклюзивный полный экран перекрыть нельзя.',
              'Overlay not showing in game? Switch the game to borderless mode — nothing can draw over exclusive fullscreen.',
            )}
          </p>
        </section>

        <section className="set-section">
          <h3>{tr('Подсказки в тюрьме', 'Prison hints')}</h3>
          <label className="set-row">
            <span
              title={tr(
                'В сезонной тюрьме (Тюрьма Талоса) рисует поверх игры стрелку к лучшему ходу. Ходы делаете вы сами.',
                'In the season prison (Tartarus) draws an arrow to the best move over the game. You make the moves yourself.',
              )}
            >
              {tr('Стрелки к лучшему ходу', 'Arrows to the best move')}
            </span>
            <input
              type="checkbox"
              checked={prisonHints}
              onChange={(e) => {
                setPrisonHints(e.target.checked);
                void window.gorn?.setPrisonHints(e.target.checked);
              }}
            />
          </label>
          <label className="set-row">
            <span
              title={tr(
                'Каждый кадр тюрьмы — PNG и описание (что за экран и какая подсказка) в «Изображения → Горн — тюрьма». Для разбора ошибок.',
                'Every prison capture — a PNG and a description (which screen, which hint) in Pictures → Горн — тюрьма. For tracking down misreads.',
              )}
            >
              {tr('Сохранять кадры', 'Save captures')}
            </span>
            <input
              type="checkbox"
              checked={prisonShots}
              onChange={(e) => {
                setPrisonShots(e.target.checked);
                void window.gorn?.setPrisonSaveShots(e.target.checked);
              }}
            />
          </label>
          <div className="set-row">
            <span>{tr('Папка с кадрами', 'Captures folder')}</span>
            <button type="button" onClick={() => void window.gorn?.openPrisonShots()}>
              {tr('Открыть', 'Open')}
            </button>
          </div>
          <p className="muted small">
            {tr(
              'Единственная функция Горна, которая смотрит на экран: в тюрьме по строкам лога снимается экран с игрой, распознаётся доска и считается ход. Снимки никуда не уходят и сохраняются, только если включено «Сохранять кадры». Нужен режим «окно без рамки» на весь монитор. Ctrl+Alt+P — снять кадр вручную.',
              'The one Gorn feature that looks at the screen: in the prison the screen with the game is captured on log lines, the board is read and a move is worked out. Captures are never sent anywhere and are saved only with “Save captures” on. Needs borderless fullscreen. Ctrl+Alt+P captures by hand.',
            )}
          </p>
        </section>

        <section className="set-section">
          <h3>{tr('Язык', 'Language')}</h3>
          <div className="set-row">
            <span>{tr('Интерфейс', 'Interface')}</span>
            <UiLanguageSwitch />
          </div>
          <div className="set-row">
            <span>{tr('Названия предметов', 'Item names')}</span>
            <NameLanguageSwitch />
          </div>
          <label className="set-row">
            <span
              title={tr(
                'Первичные цены из базы сообщества TITrack (titrack.ninja). Свои пробивки всегда в приоритете.',
                'Baseline prices from TITrack community prices (titrack.ninja). Your own price checks always win.',
              )}
            >
              {tr('Цены сообщества', 'Community prices')}
            </span>
            <input
              type="checkbox"
              checked={community}
              onChange={(e) => {
                setCommunity(e.target.checked);
                void window.gorn?.setCommunityPrices(e.target.checked);
              }}
            />
          </label>
          <label className="set-row">
            <span
              title={tr(
                'Строки дешевле порога в списках добычи сворачиваются в одну строку «мелочь». На подсчёт дохода не влияет.',
                'Loot rows below the threshold collapse into one “small stuff” row. Income totals are not affected.',
              )}
            >
              {tr('Сворачивать мелочь', 'Collapse small stuff')}
            </span>
            <select
              value={String(minDropFe)}
              onChange={(e) => void window.gorn?.setMinDropFe(Number(e.target.value))}
            >
              <option value="0">{tr('не сворачивать', 'don’t collapse')}</option>
              <option value="0.1">{tr('дешевле 0,1 ФЕ', 'under 0.1 FE')}</option>
              <option value="1">{tr('дешевле 1 ФЕ', 'under 1 FE')}</option>
              <option value="5">{tr('дешевле 5 ФЕ', 'under 5 FE')}</option>
              {![0, 0.1, 1, 5].includes(minDropFe) && (
                <option value={String(minDropFe)}>
                  {tr(`дешевле ${minDropFe} ФЕ`, `under ${minDropFe} FE`)}
                </option>
              )}
            </select>
          </label>
        </section>

        <section className="set-section">
          <h3>{tr('Данные', 'Data')}</h3>
          <div className="set-dir">
            <span className="muted small">{tr('Папка логов:', 'Log folder:')}</span>
            <span className="set-path" title={sys?.resolvedLogDir ?? undefined}>
              {sys?.logDir ?? sys?.resolvedLogDir ?? tr('не найдена', 'not found')}
              {!sys?.logDir && sys?.resolvedLogDir && (
                <span className="muted"> · {tr('найдена автоматически', 'found automatically')}</span>
              )}
            </span>
          </div>
          <div className="set-actions">
            <button className="set-btn" onClick={chooseDir}>
              {tr('Указать папку…', 'Choose folder…')}
            </button>
            {sys?.logDir && (
              <button className="set-btn" onClick={resetDir}>
                {tr('Вернуть автопоиск', 'Back to auto-detect')}
              </button>
            )}
          </div>
          {logDirNote && <p className="note warn">{logDirNote}</p>}
          {needsRestart && (
            <p className="note warn">
              {tr('Папка сменится после перезапуска.', 'The folder will change after a restart.')}{' '}
              <button className="set-btn" onClick={() => window.gorn?.settings.relaunch()}>
                {tr('Перезапустить сейчас', 'Restart now')}
              </button>
            </p>
          )}
        </section>

        <section className="set-section">
          <h3>{tr('Система', 'System')}</h3>
          <label className="set-row">
            <span>{tr('Запускать вместе с Windows', 'Start with Windows')}</span>
            <input
              type="checkbox"
              checked={sys?.autoStart ?? false}
              onChange={(e) => {
                void window.gorn?.settings.setAutoStart(e.target.checked).then(refreshSys);
              }}
            />
          </label>
          <label className="set-row">
            <span
              title={tr(
                'Закрытие окна сворачивает трекер в трей — оверлей и подсчёт продолжают работать',
                'Closing the window sends the tracker to the tray — the overlay and tracking keep running',
              )}
            >
              {tr('Закрывать в трей', 'Close to tray')}
            </span>
            <input
              type="checkbox"
              checked={sys?.closeToTray ?? false}
              onChange={(e) => {
                void window.gorn?.settings.setCloseToTray(e.target.checked).then(refreshSys);
              }}
            />
          </label>
          <div className="set-row">
            <span className="muted">
              {tr('Горн', 'Gorn')} {sys?.version ? `v${sys.version}` : ''}
            </span>
            <span className="set-update">
              {updateNote && <span className="muted small">{updateNote}</span>}
              <button
                className="set-btn"
                onClick={() => {
                  setUpdChecked(true);
                  void window.gorn?.update.check();
                }}
              >
                {tr('Проверить обновления', 'Check for updates')}
              </button>
            </span>
          </div>
        </section>
        {children}
      </aside>
    </>
  );
}

/** Строка порога звука находки: «выключен» или ступень в ФЕ (нестандартное значение тоже видно). */
function ThresholdRow(props: {
  label: string;
  hint: string;
  value: number;
  steps: readonly number[];
  disabled: boolean;
  onChange: (fe: number) => void;
}) {
  const { label, hint, value, steps, disabled, onChange } = props;
  return (
    <label className={`set-row ${disabled ? 'set-disabled' : ''}`} title={hint}>
      <span>{label}</span>
      <select value={String(value)} disabled={disabled} onChange={(e) => onChange(Number(e.target.value))}>
        <option value="0">{tr('выключен', 'off')}</option>
        {steps.map((fe) => (
          <option key={fe} value={String(fe)}>
            {tr(`${fe} ФЕ`, `${fe} FE`)}
          </option>
        ))}
        {value > 0 && !steps.includes(value) && <option value={String(value)}>{tr(`${value} ФЕ`, `${value} FE`)}</option>}
      </select>
    </label>
  );
}
