// Цены слотов сумки поверх «Моего прилавка»: прозрачное окно во весь монитор,
// клики насквозь (формат — app/src/stall.rs, StallLayer::show и labels).
//
// На каждой занятой клетке сверху — плашка во всю ширину с ценой стопки.
// Цвет заливки — ступень цены стопки (TIERS): дешевле 10 ФЕ — белым по
// тёмному, дальше синий, фиолетовый, оранжевый, золото. Шрифт — от размера клетки.
// Количество игры в правом нижнем углу не закрывается. Над сеткой — итог.
const root = document.getElementById('root');
const nf = new Intl.NumberFormat('ru-RU');

function short(fe) {
  if (fe >= 10000) return `${nf.format(Math.round(fe / 100) / 10)}k`.replace(/\s/g, ' ');
  const digits = fe >= 10 || fe === 0 ? 0 : fe >= 1 ? 1 : 2;
  return new Intl.NumberFormat('ru-RU', { minimumFractionDigits: digits, maximumFractionDigits: digits })
    .format(fe)
    .replace(/\s/g, ' ');
}

/** Ступени цены стопки, ФЕ: от порога и выше — этот цвет. */
const TIERS = [
  [200, 't4'],
  [50, 't3'],
  [20, 't2'],
  [10, 't1'],
];

function tier(fe) {
  if (fe === null) return 'none';
  return TIERS.find(([min]) => fe >= min)?.[1] ?? 'plain';
}

function render(p) {
  root.textContent = '';
  if (!p) return;
  for (const c of p.cells) {
    const [x0, y0, x1, y1] = c.box;
    const div = document.createElement('div');
    div.className = 'stall-cell';
    div.style.left = `${x0}px`;
    div.style.top = `${y0}px`;
    div.style.width = `${x1 - x0}px`;
    div.style.height = `${y1 - y0}px`;
    div.style.fontSize = `${Math.max(14, Math.round((x1 - x0) * 0.22))}px`;
    const chip = document.createElement('span');
    if (!c.known && c.fe === null) {
      chip.className = 'stall-chip t-none nosale';
      chip.textContent = 'Не для продажи';
    } else {
      chip.className = `stall-chip t-${tier(c.fe)}`;
      chip.textContent = c.fe === null ? '?' : short(c.fe);
    }
    div.appendChild(chip);
    root.appendChild(div);
  }
  const total = document.createElement('div');
  total.className = 'stall-total';
  total.style.left = `${p.totalAt[0]}px`;
  total.style.top = `${p.totalAt[1]}px`;
  total.textContent = `вкладка: ${short(p.total)} ФЕ${p.unpriced > 0 ? ` · без цены ${p.unpriced}` : ''}`;
  if (p.cells.length > 0) {
    const [x0, , x1] = p.cells[0].box;
    total.style.fontSize = `${Math.max(16, Math.round((x1 - x0) * 0.24))}px`;
  }
  root.appendChild(total);
}

const { listen, emit } = window.__TAURI__.event;
listen('stall:prices', (e) => {
  try {
    render(e.payload);
  } catch (err) {
    console.error(err);
    root.textContent = '';
  }
});
emit('stall:ready');
