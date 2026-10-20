import './bridge.js';
import { StrictMode, useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';

import { tr } from '../core/i18n.js';
import { initLanguage } from './language.js';
import './prison.css';

/** В обычном браузере (без Electron) — пример подсказки, для вёрстки. */
const DEMO: GornPrisonHint = {
  text: 'Иди в №6',
  marks: [
    { kind: 'arrow', from: [260, 160], to: [860, 160] },
    { kind: 'ring', at: [860, 160], r: 56 },
    { kind: 'box', box: [760, 430, 960, 560] },
  ],
  live: true,
  phase: 'instant',
  anchor: [860, 90],
};

/**
 * Окно стрелок поверх игры: прозрачное, во весь монитор, клики насквозь.
 * Рисует то, что прислал main-процесс: стрелку от надзирателя к камере,
 * кольцо на цели, рамку на карточке, которую нажать, и подпись хода.
 */
function PrisonApp() {
  const [hint, setHint] = useState<GornPrisonHint | null>(null);
  // null — считаю; true — проверено вариантами; false — готово (один ход или
  // проверять было нечего).
  const [checked, setChecked] = useState<boolean | null>(null);

  useEffect(() => {
    if (!window.gorn) {
      setHint(DEMO);
      return;
    }
    const off1 = window.gorn.prison.onHint((h) => {
      setHint(h);
      setChecked(h && h.phase === 'final' ? h.live : null);
    });
    const off2 = window.gorn.prison.onSettled((live) => setChecked(live));
    window.gorn.prison.ready();
    return () => {
      off1();
      off2();
    };
  }, []);

  if (!hint) return null;
  const [ax, ay] = hint.anchor;
  if (hint.thinking) {
    return (
      <div className="prison-label prison-thinking" style={{ left: ax, top: ay }}>
        <span className="prison-spinner" aria-hidden="true" />
        <span>{tr('Горн смотрит…', 'Gorn is looking…')}</span>
      </div>
    );
  }
  return (
    <>
      {/* Новый ход — новые метки и вспышки заново; тот же ход («проверено») — без вспышек. */}
      <svg key={`${hint.text}|${JSON.stringify(hint.marks)}`} className="prison-marks" width="100%" height="100%">
        <defs>
          <marker id="head" viewBox="0 0 10 10" refX="7" refY="5" markerWidth="5" markerHeight="5" orient="auto-start-reverse">
            <path d="M0,0 L10,5 L0,10 z" className="prison-head" />
          </marker>
        </defs>
        {hint.marks.map((m, i) => {
          if (m.kind === 'arrow') {
            const [x0, y0] = m.from as [number, number];
            const [x1, y1] = m.to as [number, number];
            // Стрелка кончается у края кольца, а не в его центре.
            const len = Math.hypot(x1 - x0, y1 - y0) || 1;
            const cut = Math.min(len * 0.4, 40);
            const ex = x1 - ((x1 - x0) / len) * cut;
            const ey = y1 - ((y1 - y0) / len) * cut;
            return (
              <g key={i}>
                <line className="prison-arrow-shadow" x1={x0} y1={y0} x2={ex} y2={ey} />
                <line className="prison-arrow" x1={x0} y1={y0} x2={ex} y2={ey} markerEnd="url(#head)" />
              </g>
            );
          }
          if (m.kind === 'ring') {
            const [x, y] = m.at as [number, number];
            return <circle key={i} className="prison-ring" cx={x} cy={y} r={m.r} />;
          }
          const [x0, y0, x1, y1] = m.box as [number, number, number, number];
          return <rect key={i} className="prison-box" x={x0} y={y0} width={x1 - x0} height={y1 - y0} rx={8} />;
        })}
      </svg>
      <div className="prison-label" style={{ left: ax, top: ay }}>
        <span>{hint.text}</span>
        <small>
          {checked === null
            ? tr('предварительно — проверяю', 'first pick — checking')
            : checked
              ? tr('проверено', 'checked')
              : tr('готово', 'ready')}
        </small>
      </div>
    </>
  );
}

const root = document.getElementById('root');
if (!root) throw new Error('Не найден корневой элемент');

void initLanguage().then(() => {
  document.title = tr('Горн — подсказки в тюрьме', 'Gorn — prison hints');
  createRoot(root).render(
    <StrictMode>
      <PrisonApp />
    </StrictMode>,
  );
});
