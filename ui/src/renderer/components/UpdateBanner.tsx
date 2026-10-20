import { useEffect, useState } from 'react';

import { tr } from '../../core/i18n.js';
import type { UpdateState } from '../../main/updater.js';

/**
 * Полоса обновления.
 *
 * Показывается только когда есть что сообщить. Загрузка начинается
 * по кнопке, а не сама: трафик игрока — его дело. Установка тоже по кнопке,
 * потому что перезапуск посреди фарма недопустим.
 */
export function UpdateBanner() {
  const [state, setState] = useState<UpdateState | null>(null);
  const [dismissed, setDismissed] = useState(false);

  useEffect(() => {
    if (!window.gorn) return;
    void window.gorn.update.get().then(setState);
    return window.gorn.update.onState(setState);
  }, []);

  // «Скрыть» относится к конкретному сообщению: сменился статус — полоса
  // имеет право показаться снова.
  useEffect(() => {
    setDismissed(false);
  }, [state?.status]);

  if (!state || state.status === 'idle' || state.status === 'checking') return null;
  if (dismissed) return null;

  // Неудачная проверка обновлений — не событие для игрока: приложение
  // работает и без неё. Одна приглушённая строка; техножаргон ошибки —
  // только в подсказке, для любопытных.
  if (state.status === 'error') {
    return (
      <div className="update update-quiet">
        <span title={state.error ?? undefined}>
          {tr(
            'Обновления не проверились — нет сети? Приложение работает как обычно.',
            'Couldn’t check for updates — no network? The app works as usual.',
          )}
        </span>
        <button
          className="update-close"
          title={tr('Скрыть', 'Dismiss')}
          onClick={() => setDismissed(true)}
        >
          ✕
        </button>
      </div>
    );
  }

  if (state.status === 'downloading') {
    return (
      <div className="update">
        <span>
          {tr(`Загружаю версию ${state.version}…`, `Downloading version ${state.version}…`)}
        </span>
        <div className="update-bar">
          <i style={{ width: `${Math.round(state.progress * 100)}%` }} />
        </div>
        <span className="muted">{Math.round(state.progress * 100)}%</span>
      </div>
    );
  }

  if (state.status === 'ready') {
    return (
      <div className="update">
        <span>
          {tr(
            `Версия ${state.version} готова к установке.`,
            `Version ${state.version} is ready to install.`,
          )}
        </span>
        <button onClick={() => window.gorn.update.install()}>{tr('Перезапустить', 'Restart')}</button>
        <span className="muted small">
          {tr(
            'Или просто закройте приложение — обновится само.',
            'Or just close the app — it will update on its own.',
          )}
        </span>
      </div>
    );
  }

  // available
  return (
    <div className="update">
      <span>{tr(`Вышла версия ${state.version}.`, `Version ${state.version} is out.`)}</span>
      {state.supported ? (
        <button onClick={() => void window.gorn.update.download()}>
          {tr('Загрузить', 'Download')}
        </button>
      ) : (
        <>
          <span className="muted small">
            {tr('Портативная версия не обновляется сама —', 'The portable version doesn’t self-update —')}{' '}
            <a
              href="https://github.com/lanqvist/gorn-tli/releases/latest"
              target="_blank"
              rel="noreferrer"
            >
              {tr('скачайте новую с GitHub', 'download the new one from GitHub')}
            </a>
            .
          </span>
          {/* Внутри приложения с этой новостью сделать нечего — полосе нужен способ уйти. */}
          <button
            className="update-close"
            title={tr('Скрыть', 'Dismiss')}
            onClick={() => setDismissed(true)}
          >
            ✕
          </button>
        </>
      )}
    </div>
  );
}
