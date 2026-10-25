// Подсказки Vorax поверх игры: прозрачное окно во весь монитор, клики насквозь
// (формат — app/src/vorax_layer.rs и crates/gorn-vorax-scan/src/marks.rs).
//
// Обводка карты, которую брать (повёрнута, как карта в веере), или кнопки
// обновления; стёкла баков-целей подсвечены и обведены; сверху по центру —
// плашка с ходом и долей симуляций, доходящих до 840k. Пока доска меняется
// (dim), отметок нет, плашка приглушена.
const root = document.getElementById('root');
const SVG = 'http://www.w3.org/2000/svg';

function el(name, attrs) {
  const e = document.createElementNS(SVG, name);
  for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, v);
  return e;
}

/** Обводка: тёмная кромка, поверх — светящаяся линия. */
function outline(svg, attrs, s, fill) {
  if (fill) svg.appendChild(el('rect', { ...attrs, class: 'vorax-tank' }));
  svg.appendChild(el('rect', { ...attrs, class: 'vorax-edge', 'stroke-width': 8 * s }));
  svg.appendChild(el('rect', { ...attrs, class: 'vorax-line', 'stroke-width': 4 * s }));
}

function render(p) {
  root.textContent = '';
  if (!p) return;
  // размеры — от высоты экрана, как интерфейс игры (1440p — 1)
  const s = Math.max(0.6, p.scale || 1);
  const plan = p.plan;
  if (!p.dim) {
    const svg = el('svg', { class: 'vorax-marks', width: '100%', height: '100%' });
    if (plan.card) {
      const c = plan.card;
      const pad = 8 * s;
      const g = el('g', { transform: `translate(${c.cx} ${c.cy}) rotate(${(c.angle * 180) / Math.PI})` });
      outline(g, { x: -c.w / 2 - pad, y: -c.h / 2 - pad, width: c.w + 2 * pad, height: c.h + 2 * pad, rx: 16 * s }, s);
      svg.appendChild(g);
    }
    if (plan.button) {
      const b = plan.button;
      outline(svg, { x: b.x, y: b.y, width: b.w, height: b.h, rx: 12 * s }, s);
    }
    for (const t of plan.tanks) {
      const pad = 6 * s;
      outline(svg, { x: t.x - pad, y: t.y - pad, width: t.w + 2 * pad, height: t.h + 2 * pad, rx: 18 * s }, s, true);
    }
    root.appendChild(svg);
  }
  if (plan.pill) {
    const pill = document.createElement('div');
    pill.className = `vorax-pill${p.dim ? ' dim' : ''}`;
    pill.style.top = `${14 * s}px`;
    pill.style.fontSize = `${23 * s}px`;
    const mark = document.createElement('span');
    mark.className = 'mark';
    const main = document.createElement('span');
    main.className = 'main';
    main.textContent = plan.pill.main;
    pill.append(mark, main);
    // «33% до 840k»: доля — цветом подсказки, остальное — светло-серым
    const sub = plan.pill.sub || '';
    const m = sub.match(/^(\d+%)(.*)$/);
    if (m) {
      const odds = document.createElement('span');
      odds.className = 'odds';
      odds.textContent = m[1];
      const rest = document.createElement('span');
      rest.className = 'rest';
      rest.textContent = m[2];
      const box = document.createElement('span');
      box.append(odds, rest);
      pill.appendChild(box);
    } else if (sub) {
      const rest = document.createElement('span');
      rest.className = 'rest';
      rest.textContent = sub;
      pill.appendChild(rest);
    }
    root.appendChild(pill);
  }
}

const { listen, emit } = window.__TAURI__.event;
listen('vorax:marks', (e) => {
  try {
    render(e.payload);
  } catch (err) {
    console.error(err);
    root.textContent = '';
  }
});
emit('vorax:ready');
