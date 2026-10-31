// Окно стрелок поверх игры: прозрачное, во весь монитор, клики насквозь
// (перенос prison.tsx — держать их одинаковыми; формат подсказки —
// app/src/overlay.rs, ArrowLayer::show).
//
// Метки — по шагам хода: зелёным то, что жать сейчас (step 1), белым
// пунктиром с номером — следующий клик (2), тонким пунктиром — куда придёт
// надзиратель и кто кого ест (0). Подпись — плашка у той кнопки, которую
// жать. Пока ход считается, рамка плашки пунктирная; ход готов — сплошная.
//
// Тепловизор (Ctrl+Alt+H, только здесь, в prison.tsx его нет): на 5 секунд
// доска темнеет, варианты хода проступают цветом каления по оценке решателя.
const root = document.getElementById('root');
const SVG = 'http://www.w3.org/2000/svg';
let hint = null;
// null — считаю; true/false — ход больше не сменится.
let checked = null;
let lastKey = '';
// Тепловизор включён до этого времени (мс); 0 — выключен.
let thermoUntil = 0;
let thermoTimer = 0;
const THERMO_MS = 5000;

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

// Шкала каления — та же, что у жара темпа в оверлее (ui/src/renderer/overlay/heat.ts).
const STOPS = [
  [0, [0x4d, 0x47, 0x44]],
  [0.2, [0x6e, 0x1a, 0x10]],
  [0.38, [0xb3, 0x26, 0x1e]],
  [0.55, [0xff, 0x7a, 0x1a]],
  [0.75, [0xff, 0xc9, 0x3c]],
  [1, [0xff, 0xf3, 0xd6]],
];

function heatColor(t) {
  const k = Math.max(0, Math.min(1, t));
  for (let i = 1; i < STOPS.length; i++) {
    const [t1, c1] = STOPS[i];
    if (k <= t1) {
      const [t0, c0] = STOPS[i - 1];
      const f = (k - t0) / (t1 - t0);
      return `rgb(${c0.map((v, j) => Math.round(v + (c1[j] - v) * f)).join(',')})`;
    }
  }
  return 'rgb(255,243,214)';
}

/** Метка оценки: чип с цифрой цветом каления. */
function heatLabel(svg, x, y, text, color, best) {
  const w = 13 * text.length + 22;
  svg.appendChild(el('rect', { x: x - w / 2, y: y - 15, width: w, height: 30, rx: 8, class: 'prison-heat-chip', stroke: color }, true));
  const t = el('text', { x, y, dy: '0.35em', 'text-anchor': 'middle', fill: color, class: best ? 'prison-heat-label best' : 'prison-heat-label' }, true);
  t.textContent = text;
  svg.appendChild(t);
}

/** Слой тепловизора — между метками и плашкой; перерисовка не трогает подсказку. */
function drawThermo() {
  root.querySelector('.prison-thermo')?.remove();
  if (!hint || hint.thinking || Date.now() >= thermoUntil || !hint.heat || hint.heat.length === 0) return;
  const W = innerWidth;
  const H = innerHeight;
  const svg = el('svg', { class: 'prison-thermo', width: '100%', height: '100%' }, true);
  const defs = el('defs', {}, true);
  const blur = el('filter', { id: 'thermo-blur', x: '-30%', y: '-30%', width: '160%', height: '160%' }, true);
  blur.appendChild(el('feGaussianBlur', { stdDeviation: 10 }, true));
  defs.appendChild(blur);
  svg.appendChild(defs);
  // Доска остывает: синеватая темнота поверх игры.
  svg.appendChild(el('rect', { width: '100%', height: '100%', class: 'prison-thermo-dim' }, true));
  hint.heat.forEach((h, i) => {
    const c = heatColor(h.t);
    const best = h.t >= 1;
    const text = `${h.label}${h.near ? ' ≈' : ''}`;
    if (h.cell) {
      const [x, y] = h.cell.at;
      const r = h.cell.r;
      const g = el('radialGradient', { id: `thermo-g${i}` }, true);
      g.appendChild(el('stop', { offset: '0%', 'stop-color': c, 'stop-opacity': 0.42 }, true));
      g.appendChild(el('stop', { offset: '55%', 'stop-color': c, 'stop-opacity': 0.5 }, true));
      g.appendChild(el('stop', { offset: '70%', 'stop-color': c, 'stop-opacity': 0.3 }, true));
      g.appendChild(el('stop', { offset: '100%', 'stop-color': c, 'stop-opacity': 0 }, true));
      defs.appendChild(g);
      svg.appendChild(el('circle', { cx: x, cy: y, r: r * 1.7, fill: `url(#thermo-g${i})`, class: 'prison-thermo-glow' }, true));
      svg.appendChild(el('circle', { cx: x, cy: y, r: r * 1.03, fill: 'none', stroke: c, 'stroke-width': 3 }, true));
      // Метка — в сторону двора: под верхним рядом, над нижним, внутрь у боковых.
      const [lx, ly] =
        y < H * 0.2 ? [x, y + r + 30] : y > H * 0.45 ? [x, y - r - 30] : [x < W / 2 ? x + r + 64 : x - r - 64, y];
      heatLabel(svg, lx, ly, text, c, best);
    }
    if (h.card) {
      const [x0, y0, x1, y1] = h.card;
      svg.appendChild(el('rect', { x: x0, y: y0, width: x1 - x0, height: y1 - y0, rx: 12, fill: 'none', stroke: c, 'stroke-width': 12, filter: 'url(#thermo-blur)', class: 'prison-thermo-glow' }, true));
      svg.appendChild(el('rect', { x: x0, y: y0, width: x1 - x0, height: y1 - y0, rx: 12, fill: 'none', stroke: c, 'stroke-width': 3 }, true));
      // У карточки без камеры метка — внутри, сверху: под карточкой стоит плашка подсказки.
      if (!h.cell) heatLabel(svg, (x0 + x1) / 2, y0 + 28, text, c, best);
    }
  });
  const plate = root.querySelector('.prison-plate');
  root.insertBefore(svg, plate);
}

function render() {
  const key = hint ? `${hint.title}|${hint.sub}|${JSON.stringify(hint.marks)}` : '';
  // Новый ход — новые метки и вспышки заново; тот же ход («проверено») — только рамка плашки.
  if (key === lastKey && hint && !hint.thinking) {
    const p = root.querySelector('.prison-plate');
    if (p) p.classList.toggle('wait', checked === null);
    // Тот же ход, оценки досчитаны: тепловизор — с новыми цифрами.
    drawThermo();
    return;
  }
  lastKey = key;
  root.textContent = '';
  if (!hint) return;
  if (hint.thinking) {
    const div = plateDiv(hint.plate, 'prison-thinking');
    div.appendChild(el('span', { class: 'prison-spinner', 'aria-hidden': 'true' }));
    const t = el('span');
    t.textContent = hint.label || 'Горн смотрит…';
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
  drawThermo();
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
// Ctrl+Alt+H: тепловизор на 5 секунд; повторное нажатие — выключить раньше.
listen('prison:thermo', () => {
  clearTimeout(thermoTimer);
  if (Date.now() < thermoUntil) {
    thermoUntil = 0;
  } else {
    thermoUntil = Date.now() + THERMO_MS;
    thermoTimer = setTimeout(() => {
      thermoUntil = 0;
      drawThermo();
    }, THERMO_MS);
  }
  drawThermo();
});
listen('prison:settled', (e) => {
  checked = e.payload;
  render();
});
emit('prison:ready');
