// Окно стрелок поверх игры: прозрачное, во весь монитор, клики насквозь.
// Рисует то, что прислал Горн: стрелку от надзирателя к камере, кольцо на
// цели, рамку на карточке, которую нажать, и подпись хода (перенос prison.tsx).
const root = document.getElementById('root');
const SVG = 'http://www.w3.org/2000/svg';
let hint = null;
// null — считаю; true — проверено вариантами; false — готово.
let checked = null;
let lastKey = '';

function el(tag, attrs, ns) {
  const e = ns ? document.createElementNS(SVG, tag) : document.createElement(tag);
  for (const [k, v] of Object.entries(attrs || {})) e.setAttribute(k, String(v));
  return e;
}

function label(ax, ay, cls) {
  const div = el('div', { class: cls });
  div.style.left = `${ax}px`;
  div.style.top = `${ay}px`;
  return div;
}

function render() {
  const key = hint ? `${hint.text}|${JSON.stringify(hint.marks)}` : '';
  // Новый ход — новые метки и вспышки заново; тот же ход («проверено») — только подпись.
  if (key === lastKey && hint && !hint.thinking) {
    const small = root.querySelector('.prison-label small');
    if (small) small.textContent = status();
    return;
  }
  lastKey = key;
  root.textContent = '';
  if (!hint) return;
  const [ax, ay] = hint.anchor;
  if (hint.thinking) {
    const div = label(ax, ay, 'prison-label prison-thinking');
    div.appendChild(el('span', { class: 'prison-spinner', 'aria-hidden': 'true' }));
    const t = el('span');
    t.textContent = 'Горн смотрит…';
    div.appendChild(t);
    root.appendChild(div);
    return;
  }
  const svg = el('svg', { class: 'prison-marks', width: '100%', height: '100%' }, true);
  const defs = el('defs', {}, true);
  const marker = el('marker', { id: 'head', viewBox: '0 0 10 10', refX: 7, refY: 5, markerWidth: 5, markerHeight: 5, orient: 'auto-start-reverse' }, true);
  marker.appendChild(el('path', { d: 'M0,0 L10,5 L0,10 z', class: 'prison-head' }, true));
  defs.appendChild(marker);
  svg.appendChild(defs);
  for (const m of hint.marks) {
    if (m.kind === 'arrow') {
      const [x0, y0] = m.from;
      const [x1, y1] = m.to;
      // Стрелка кончается у края кольца, а не в его центре.
      const len = Math.hypot(x1 - x0, y1 - y0) || 1;
      const cut = Math.min(len * 0.4, 40);
      const ex = x1 - ((x1 - x0) / len) * cut;
      const ey = y1 - ((y1 - y0) / len) * cut;
      svg.appendChild(el('line', { class: 'prison-arrow-shadow', x1: x0, y1: y0, x2: ex, y2: ey }, true));
      svg.appendChild(el('line', { class: 'prison-arrow', x1: x0, y1: y0, x2: ex, y2: ey, 'marker-end': 'url(#head)' }, true));
    } else if (m.kind === 'ring') {
      svg.appendChild(el('circle', { class: 'prison-ring', cx: m.at[0], cy: m.at[1], r: m.r }, true));
    } else {
      const [x0, y0, x1, y1] = m.box;
      svg.appendChild(el('rect', { class: 'prison-box', x: x0, y: y0, width: x1 - x0, height: y1 - y0, rx: 8 }, true));
    }
  }
  root.appendChild(svg);
  const div = label(ax, ay, 'prison-label');
  const t = el('span');
  t.textContent = hint.text;
  const small = el('small');
  small.textContent = status();
  div.appendChild(t);
  div.appendChild(small);
  root.appendChild(div);
}

function status() {
  return checked === null ? 'предварительно — проверяю' : checked ? 'проверено' : 'готово';
}

const { listen, emit } = window.__TAURI__.event;
listen('prison:hint', (e) => {
  hint = e.payload;
  checked = hint && hint.phase === 'final' ? hint.live : null;
  render();
});
listen('prison:settled', (e) => {
  checked = e.payload;
  render();
});
emit('prison:ready');
