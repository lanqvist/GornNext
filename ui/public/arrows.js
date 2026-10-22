// Окно стрелок поверх игры: прозрачное, во весь монитор, клики насквозь
// (перенос prison.tsx — держать их одинаковыми; формат подсказки —
// app/src/overlay.rs, ArrowLayer::show).
//
// Метки — по шагам хода: зелёным то, что жать сейчас (step 1), белым
// пунктиром с номером — следующий клик (2), тонким пунктиром — куда придёт
// надзиратель и кто кого ест (0). Подпись — плашка у той кнопки, которую
// жать. Пока ход считается, рамка плашки пунктирная; ход готов — сплошная.
const root = document.getElementById('root');
const SVG = 'http://www.w3.org/2000/svg';
let hint = null;
// null — считаю; true/false — ход больше не сменится.
let checked = null;
let lastKey = '';

function el(tag, attrs, ns) {
  const e = ns ? document.createElementNS(SVG, tag) : document.createElement(tag);
  for (const [k, v] of Object.entries(attrs || {})) e.setAttribute(k, String(v));
  return e;
}

function plateDiv(plate, cls) {
  const div = el('div', { class: `prison-plate anchor-${plate.anchor} ${cls}` });
  div.style.left = `${plate.at[0]}px`;
  div.style.top = `${plate.at[1]}px`;
  if (plate.card) div.style.minWidth = `${plate.card}px`;
  return div;
}

function stepClass(m) {
  return m.step === 1 ? 'now' : m.step === 2 ? 'then' : 'show';
}

/** Порядок слоёв: «только показать» внизу, следующий шаг выше, текущий — сверху. */
function layer(m) {
  if (m.kind === 'path') return 0;
  return m.step === 1 ? 3 : m.step === 2 ? 2 : 1;
}

function mark(m, marks) {
  if (m.kind === 'path') {
    const g = el('g', { class: 'prison-path' }, true);
    g.appendChild(el('polyline', { points: m.points.map((p) => p.join(',')).join(' ') }, true));
    for (const p of m.points.slice(1, -1)) g.appendChild(el('circle', { cx: p[0], cy: p[1], r: 5 }, true));
    return g;
  }
  const cls = stepClass(m);
  if (m.kind === 'arrow') {
    const [x0, y0] = m.from;
    const [x1, y1] = m.to;
    // Стрелка кончается у края кольца цели, а не в его центре.
    const ring = marks.find((o) => o.kind === 'ring' && o.at[0] === x1 && o.at[1] === y1);
    const len = Math.hypot(x1 - x0, y1 - y0) || 1;
    const cut = Math.min(len * 0.4, ring ? ring.r + 8 : 40);
    const ex = x1 - ((x1 - x0) / len) * cut;
    const ey = y1 - ((y1 - y0) / len) * cut;
    return el('line', { class: `prison-arrow ${cls}`, x1: x0, y1: y0, x2: ex, y2: ey, 'marker-end': 'url(#head)' }, true);
  }
  if (m.kind === 'ring') {
    return el('circle', { class: `prison-ring ${cls}`, cx: m.at[0], cy: m.at[1], r: m.r }, true);
  }
  const [x0, y0, x1, y1] = m.box;
  return el('rect', { class: `prison-box ${cls}`, x: x0, y: y0, width: x1 - x0, height: y1 - y0, rx: 10 }, true);
}

/** Ход из двух кликов: номер шага на метке — в углу рамки или на кольце сверху справа. */
function badge(m) {
  if (m.kind !== 'box' && m.kind !== 'ring') return null;
  const [x, y] = m.kind === 'box' ? [m.box[0], m.box[1]] : [m.at[0] + m.r * 0.72, m.at[1] - m.r * 0.72];
  const now = m.step === 1;
  const g = el('g', { class: now ? 'prison-badge now' : 'prison-badge' }, true);
  g.appendChild(el('circle', { cx: x, cy: y, r: now ? 15 : 13 }, true));
  const t = el('text', { x, y, dy: '0.36em', 'text-anchor': 'middle' }, true);
  t.textContent = String(m.step);
  g.appendChild(t);
  return g;
}

function render() {
  const key = hint ? `${hint.title}|${hint.sub}|${JSON.stringify(hint.marks)}` : '';
  // Новый ход — новые метки и вспышки заново; тот же ход («проверено») — только рамка плашки.
  if (key === lastKey && hint && !hint.thinking) {
    const p = root.querySelector('.prison-plate');
    if (p) p.classList.toggle('wait', checked === null);
    return;
  }
  lastKey = key;
  root.textContent = '';
  if (!hint) return;
  if (hint.thinking) {
    const div = plateDiv(hint.plate, 'prison-thinking');
    div.appendChild(el('span', { class: 'prison-spinner', 'aria-hidden': 'true' }));
    const t = el('span');
    t.textContent = 'Горн смотрит…';
    div.appendChild(t);
    root.appendChild(div);
    return;
  }
  const svg = el('svg', { class: 'prison-marks', width: '100%', height: '100%' }, true);
  const defs = el('defs', {}, true);
  const marker = el('marker', { id: 'head', viewBox: '0 0 10 10', refX: 6, refY: 5, markerWidth: 5, markerHeight: 5, orient: 'auto' }, true);
  marker.appendChild(el('path', { d: 'M0,0 L10,5 L0,10 z', class: 'prison-head' }, true));
  defs.appendChild(marker);
  svg.appendChild(defs);
  const marks = [...hint.marks].sort((a, b) => layer(a) - layer(b));
  for (const m of marks) svg.appendChild(mark(m, hint.marks));
  if (hint.twoSteps) {
    for (const m of marks) {
      const b = badge(m);
      if (b) svg.appendChild(b);
    }
  }
  root.appendChild(svg);

  const div = plateDiv(hint.plate, `${hint.plate.card ? 'on-card' : ''} ${checked === null ? 'wait' : ''}`);
  const title = el('div', { class: 'prison-title' });
  const t = el('span');
  t.textContent = hint.title;
  title.appendChild(t);
  if (hint.twoSteps) {
    const pill = el('span', { class: 'prison-pill' });
    pill.textContent = '1/2';
    title.appendChild(pill);
  }
  div.appendChild(title);
  if (hint.sub) {
    const small = el('small');
    small.textContent = hint.sub;
    div.appendChild(small);
  }
  root.appendChild(div);
}

const { listen, emit } = window.__TAURI__.event;
listen('prison:hint', (e) => {
  hint = e.payload;
  checked = hint && hint.phase === 'final' ? hint.live : null;
  try {
    render();
  } catch (err) {
    // Сломанная подсказка не должна глушить следующие: окно чистое, ждём новую.
    console.error(err);
    root.textContent = '';
    lastKey = '';
  }
});
listen('prison:settled', (e) => {
  checked = e.payload;
  render();
});
emit('prison:ready');
