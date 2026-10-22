import './bridge.js';
import { StrictMode, useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';

import { tr } from '../core/i18n.js';
import { initLanguage } from './language.js';
import './prison.css';

type Mark = GornPrisonHint['marks'][number];

/** В обычном браузере (без Tauri) — примеры подсказок для вёрстки: `?demo=patrol|devour|target|wait`. */
const DEVOUR: GornPrisonHint = {
  title: 'Пожирание',
  sub: 'затем №6',
  twoSteps: true,
  marks: [
    { kind: 'box', box: [621, 747, 953, 925], step: 1 },
    { kind: 'ring', at: [1392, 139], r: 70, step: 2 },
  ],
  plate: { at: [787, 939], anchor: 'top', card: 332 },
  live: true,
  phase: 'final',
};
const PATROL: GornPrisonHint = {
  title: 'Иди в №4',
  sub: 'событие',
  twoSteps: false,
  marks: [
    { kind: 'box', box: [793, 747, 1125, 925], step: 1 },
    { kind: 'ring', at: [1044, 139], r: 70, step: 0 },
    { kind: 'path', points: [[615, 251], [698, 251], [870, 251], [1044, 251]] },
  ],
  plate: { at: [959, 939], anchor: 'top', card: 332 },
  live: true,
  phase: 'final',
};
const DEMOS: Record<string, GornPrisonHint> = {
  devour: DEVOUR,
  patrol: PATROL,
  wait: { ...PATROL, phase: 'instant', live: false },
  target: {
    title: 'Съесть №6',
    sub: '',
    twoSteps: false,
    marks: [
      { kind: 'ring', at: [1392, 139], r: 70, step: 1 },
      { kind: 'arrow', from: [512, 418], to: [1392, 139], step: 0 },
    ],
    plate: { at: [1153, 269], anchor: 'top' },
    live: true,
    phase: 'final',
  },
};

/** Ход из двух кликов: номер шага на метке — в углу рамки или на кольце сверху справа. */
function Badge({ m }: { m: Mark }) {
  if (m.kind !== 'box' && m.kind !== 'ring') return null;
  const [x, y] = m.kind === 'box' ? [m.box[0], m.box[1]] : [m.at[0] + m.r * 0.72, m.at[1] - m.r * 0.72];
  const now = m.step === 1;
  return (
    <g className={now ? 'prison-badge now' : 'prison-badge'}>
      <circle cx={x} cy={y} r={now ? 15 : 13} />
      <text x={x} y={y} dy="0.36em" textAnchor="middle">
        {m.step}
      </text>
    </g>
  );
}

function MarkShape({ m, marks }: { m: Mark; marks: Mark[] }) {
  if (m.kind === 'path') {
    const pts = m.points;
    return (
      <g className="prison-path">
        <polyline points={pts.map((p) => p.join(',')).join(' ')} />
        {pts.slice(1, -1).map((p, i) => (
          <circle key={i} cx={p[0]} cy={p[1]} r={5} />
        ))}
      </g>
    );
  }
  const cls = m.step === 1 ? 'now' : m.step === 2 ? 'then' : 'show';
  if (m.kind === 'arrow') {
    const [x0, y0] = m.from;
    const [x1, y1] = m.to;
    // Стрелка кончается у края кольца цели, а не в его центре.
    const ring = marks.find((o) => o.kind === 'ring' && o.at[0] === x1 && o.at[1] === y1);
    const len = Math.hypot(x1 - x0, y1 - y0) || 1;
    const cut = Math.min(len * 0.4, ring && ring.kind === 'ring' ? ring.r + 8 : 40);
    const ex = x1 - ((x1 - x0) / len) * cut;
    const ey = y1 - ((y1 - y0) / len) * cut;
    return <line className={`prison-arrow ${cls}`} x1={x0} y1={y0} x2={ex} y2={ey} markerEnd="url(#head)" />;
  }
  if (m.kind === 'ring') {
    return <circle className={`prison-ring ${cls}`} cx={m.at[0]} cy={m.at[1]} r={m.r} />;
  }
  const [x0, y0, x1, y1] = m.box;
  return <rect className={`prison-box ${cls}`} x={x0} y={y0} width={x1 - x0} height={y1 - y0} rx={10} />;
}

/** Порядок слоёв: «только показать» внизу, следующий шаг выше, текущий — сверху. */
function layer(m: Mark): number {
  if (m.kind === 'path') return 0;
  return m.step === 1 ? 3 : m.step === 2 ? 2 : 1;
}

/**
 * Окно подсказок поверх игры: прозрачное, во весь монитор, клики насквозь.
 * Метки — по шагам хода: зелёным то, что жать сейчас, белым пунктиром с
 * номером — следующий клик, тонким пунктиром — куда придёт надзиратель и
 * кто кого ест. Подпись — плашка у той кнопки, которую жать. Пока ход
 * считается, рамка плашки пунктирная; ход готов — сплошная.
 */
function PrisonApp() {
  const [hint, setHint] = useState<GornPrisonHint | null>(null);
  // null — считаю; true/false — ход больше не сменится.
  const [checked, setChecked] = useState<boolean | null>(null);

  useEffect(() => {
    if (!window.gorn) {
      const demo = DEMOS[new URLSearchParams(location.search).get('demo') ?? 'devour'] ?? DEVOUR;
      setHint(demo);
      setChecked(demo.phase === 'final' ? demo.live : null);
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
  const { plate } = hint;
  const [px, py] = plate.at;
  if (hint.thinking) {
    return (
      <div className="prison-plate prison-thinking anchor-top" style={{ left: px, top: py }}>
        <span className="prison-spinner" aria-hidden="true" />
        <span>{tr('Горн смотрит…', 'Gorn is looking…')}</span>
      </div>
    );
  }
  const marks = [...hint.marks].sort((a, b) => layer(a) - layer(b));
  const plateCls = [
    'prison-plate',
    `anchor-${plate.anchor}`,
    plate.card ? 'on-card' : '',
    checked === null ? 'wait' : '',
  ].join(' ');
  return (
    // Новый ход — новые метки и вспышки заново; тот же ход («проверено») — без вспышек.
    <div key={`${hint.title}|${hint.sub}|${JSON.stringify(hint.marks)}`}>
      <svg className="prison-marks" width="100%" height="100%">
        <defs>
          <marker id="head" viewBox="0 0 10 10" refX="6" refY="5" markerWidth="5" markerHeight="5" orient="auto">
            <path d="M0,0 L10,5 L0,10 z" className="prison-head" />
          </marker>
        </defs>
        {marks.map((m, i) => (
          <MarkShape key={i} m={m} marks={hint.marks} />
        ))}
        {hint.twoSteps && marks.map((m, i) => <Badge key={`b${i}`} m={m} />)}
      </svg>
      <div className={plateCls} style={{ left: px, top: py, minWidth: plate.card }}>
        <div className="prison-title">
          <span>{hint.title}</span>
          {hint.twoSteps && <span className="prison-pill">1/2</span>}
        </div>
        {hint.sub && <small>{hint.sub}</small>}
      </div>
    </div>
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
