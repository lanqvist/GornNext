// Справочник снаряжения: crates/gorn-core/data/gear.json.
//
//   node tools/build-gear.mjs
//
// Дроп снаряжения в логе — номер базы (ConfigBaseId) без названия, а в выгрузке
// tlidb, из которой собран items.json, баз снаряжения нет. Словарь tlidb
// (autocomplete) знает их названия и страницы, а страница вещи — её номер
// («id: 1200»), тот же, что пишет лог. Скрипт проходит страницы снаряжения
// по одной в секунду (вежливо к чужому серверу), берёт номер и иконку,
// русское название — из русского словаря по тому же ключу страницы.
// Прерванный прогон продолжается с места: страницы кэшируются в target/tlidb.
// У легендарки страница называет и её базу («base: 4647»): в логе подбор
// легендарки — номер базы, а пробивка и сравнение — номер самой легендарки.

import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';

const root = resolve(import.meta.dirname, '..');
const cache = join(root, 'target/tlidb');
mkdirSync(cache, { recursive: true });

// Категории словаря, которые падают как снаряжение.
const GEAR = new Set([
  'Legendary', 'Ring', 'Necklace', 'Belt', 'Spirit Ring', 'Vorax Gear',
  'Claw', 'One-Handed Hammer', 'One-Handed Sword', 'One-Handed Axe', 'Dagger', 'Cane',
  'Two-Handed Hammer', 'Two-Handed Axe', 'Two-Handed Sword', 'Cudgel', 'Tin Staff',
  'Pistol', 'Bow', 'Crossbow', 'Fire Cannon', 'Musket', 'Wand', 'Rod', 'Scepter',
  ...['STR', 'DEX', 'INT'].flatMap((a) => ['Chest Armor', 'Gloves', 'Helmet', 'Boots', 'Shield'].map((p) => `${a} ${p}`)),
]);

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function json(url) {
  const r = await fetch(url);
  if (!r.ok) throw new Error(`${url}: HTTP ${r.status}`);
  return r.json();
}

/** Страница вещи из кэша или с сервера (тогда — пауза: страница в секунду). */
async function page(slug) {
  const file = join(cache, `${slug.replace(/[^\w.-]/g, '_')}.html`);
  if (existsSync(file)) return readFileSync(file, 'utf8');
  // Сервер иногда обрывает соединение — повтор с паузой, до пяти раз.
  let html = null;
  for (let attempt = 1; attempt <= 5 && html === null; attempt++) {
    try {
      const r = await fetch(`https://tlidb.com/en/${encodeURIComponent(slug)}`);
      await sleep(1000);
      if (!r.ok) return null;
      html = await r.text();
    } catch (err) {
      console.log(`  ${slug}: ${err.cause?.code ?? err.message}, повтор ${attempt}`);
      await sleep(5000 * attempt);
    }
  }
  if (html === null) return null;
  writeFileSync(file, html);
  return html;
}

const en = await json('https://tlidb.com/i18n/autocomplete_en.json');
const ru = new Map((await json('https://tlidb.com/i18n/autocomplete_ru.json')).map((x) => [x.value, x]));
const items = new Set(JSON.parse(readFileSync(join(root, 'crates/gorn-core/data/items.json'), 'utf8')).map((it) => it.id));

const wanted = en.filter((x) => GEAR.has(x.desc));
console.log(`снаряжения в словаре tlidb: ${wanted.length}`);
const out = [];
let n = 0;
for (const x of wanted) {
  n++;
  const html = await page(x.value);
  const id = html && /id:\s*(\d+)/.exec(html)?.[1];
  if (!id) {
    console.log(`  нет номера: ${x.value}`);
    continue;
  }
  const icon = /https?:\/\/cdn\.tlidb\.com\/[^"']*\/Icon[^"']*\.webp/.exec(html)?.[0] ?? null;
  const r = ru.get(x.value);
  const base = x.desc === 'Legendary' ? Number(/base:\s*(\d+)/.exec(html)?.[1]) || null : null;
  out.push({
    id: Number(id),
    name: x.label,
    nameRu: r?.label ?? null,
    type: x.desc,
    typeRu: r?.desc ?? null,
    icon,
    ...(base ? { base } : {}),
    inItems: items.has(Number(id)),
  });
  if (n % 50 === 0) console.log(`  ${n}/${wanted.length}`);
}
out.sort((a, b) => a.id - b.id);
// Повторы номера (одна вещь под двумя ключами словаря) — первая.
const seen = new Set();
const gear = out.filter((g) => !seen.has(g.id) && seen.add(g.id)).map(({ inItems, ...g }) => g);
writeFileSync(join(root, 'crates/gorn-core/data/gear.json'), JSON.stringify(gear, null, 1) + '\n');
console.log(`gear.json: ${gear.length} вещей, из них нет в items.json: ${out.filter((g) => !g.inItems).length}`);
