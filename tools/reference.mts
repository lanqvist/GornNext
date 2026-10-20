// Эталон тюрьмы для сверки и замеров: движок и мгновенная стратегия из
// соседнего репозитория gorn-tli (TypeScript), ничего в нём не меняя.
//
//   node --import <tli>/node_modules/tsx/dist/loader.mjs tools/reference.mts <tli> <команда> …
//   (или: npx --prefix ../gorn-tli tsx tools/reference.mts ../gorn-tli <команда> …)
//
// Команды (вывод — в stdout):
//   weights [hand]                  — DEFAULT_WEIGHTS (или ручные HAND): по строке «индекс hex-биты-f64»
//   rng <from> <n> <count>          — ГСЧ: на сид строка чисел (next, bits30, float, int, chance, stream)
//   runs <instant|random> <from> <n> hash
//                                   — забеги: на сид строка «сид ходов счёт h1 h2»; h1/h2 —
//                                     потоковый хеш (код хода, состояние после него) по всем ходам
//   runs <instant|random> <from> <n> full
//                                   — то же целиком, двоичный поток int32 LE: на забег
//                                     [сид, K], затем K записей [код, состояние SIZE слов];
//                                     первая запись — после startRun, код −1
//   bench <selfplay-seeds>          — замеры (текстом), см. bench() ниже
//   solve <default|free> <from> <n> <rounds> [games 18] [every 2]
//                                   — решатель на позициях prison-solve-bench (сиды 4242+g,
//                                     каждое every-е решение с ходами > 1): позиции from…from+n−1,
//                                     зерно 1000+i, потолок rounds раундов, без срока. Строка:
//                                     «i раундов done лучший k код:среднее:gapSe:раунды:отсечён …»
//                                     (дробные — hex-биты f64)
//   play <from> <n> <rounds>        — самоигра с решателем без срока (как gorn-cli play --mode fixed):
//                                     сид 777000+g, тема g % 5, цель случайная; ход решателя —
//                                     solve({seed: g·1000+ход, maxRounds: rounds}).step(∞), где
//                                     ходов > 1. Строка: «g мгновенная решатель h1 h2» (хеш ходов решателя)
//   scores <random|greedy|policy> <seed> <n>
//                                   — забеги prison-tune (сценарий «все темы по кругу», кости
//                                     imul(seed ^ i, 0x9e3779b1) ^ i): «n среднее h1 h2» по счётам
//                                     (policy — настроенные веса, остальные — ручные)
//   read <fresh|chain|chainre> <список>
//                                   — распознавание кадров (PNG, пути по строке в файле-списке):
//                                     fresh — каждый кадр без прежней геометрии; chain — геометрия
//                                     переходит от кадра к кадру по порядку списка; chainre — то же
//                                     с recalibrate. Строка — JSON {file, view, geo}
//   selfplay <from> <n>             — самоигра instant-сидов без записи: «забегов ходов сумма-призм.»
//                                     (для замера на нескольких процессах)
//
// Забег сида g (одинаково в gorn-cli verify):
//   instant — Rng(g), тема START_EVENT_IDS[g % 5], цель OBJECTIVE_IDS[⌊g / 5⌋ % 5], выбор орудия
//             в начале, ходы — instantCode, применение — applyCode с проверкой;
//   random  — Rng(g), тема g % 5, цель случайная (бросок движка), орудия по g % 4: нет / 6 из
//             prison-bench / все 31 / маска из Rng(g ^ 0x5bd1e995).next(); ходы — случайные
//             легальные: buf[r.int(n)], как randomPlayout.
import { once } from 'node:events';
import { pathToFileURL } from 'node:url';

const [tli, cmd, ...rest] = process.argv.slice(2);
if (!tli || !cmd) {
  console.error('reference.mts <gorn-tli> <weights|rng|runs|bench> …');
  process.exit(2);
}
const root = pathToFileURL(`${tli}/src/`).href;
const imp = (p: string) => import(new URL(p, root).href);
const model = await imp('core/prison/model.ts');
const actions = await imp('core/prison/engine/actions.ts');
const { Rng } = await imp('core/prison/engine/rng.ts');
const st = await imp('core/prison/engine/state.ts');
const { instantCode, policyPlayout } = await imp('core/prison/policy/instant.ts');
const { DEFAULT_WEIGHTS } = await imp('core/prison/policy/weights.ts');
const solver = await imp('core/prison/solver/solve.ts');
const { greedyPlayout } = await imp('core/prison/policy/greedy.ts');
const { HAND, fromJson } = await imp('core/prison/policy/weights.ts');

const p = model.DEFAULT_PARAMS;
const SIZE: number = st.SIZE;
const SIX = ['keys', 'coin', 'trap', 'claws', 'ravenous', 'scales'];
const ALL_TOOLS = 0x7fffffff;

// ── вывод с учётом обратного давления ───────────────────────────────────────

let lines: string[] = [];
async function line(s: string): Promise<void> {
  lines.push(s);
  if (lines.length >= 2000) await flushLines();
}
async function flushLines(): Promise<void> {
  if (lines.length === 0) return;
  const text = lines.join('\n') + '\n';
  lines = [];
  if (!process.stdout.write(text)) await once(process.stdout, 'drain');
}
async function writeBin(a: Int32Array, n: number): Promise<void> {
  const buf = Buffer.from(a.buffer, a.byteOffset, n * 4);
  // копия: буфер переиспользуется, а запись асинхронная
  if (!process.stdout.write(Buffer.from(buf))) await once(process.stdout, 'drain');
}

// ── потоковый хеш (тот же — StreamHash в crates/gorn-prison/src/state.rs) ────

let h1 = 0, h2 = 0;
function hReset(): void {
  h1 = 0x811c9dc5 | 0;
  h2 = 0x9747b28c | 0;
}
function hPush(v: number): void {
  h1 = Math.imul(h1 ^ v, 0x01000193);
  const x = Math.imul(h2 ^ v, 0x5bd1e995);
  h2 = x ^ (x >>> 15);
}
function hState(m: Int32Array): void {
  for (let i = 0; i < SIZE; i++) {
    h1 = Math.imul(h1 ^ m[i]!, 0x01000193);
    const x = Math.imul(h2 ^ m[i]!, 0x5bd1e995);
    h2 = x ^ (x >>> 15);
  }
}

// ── забеги ──────────────────────────────────────────────────────────────────

function toolsFor(g: number): string[] {
  switch (g % 4) {
    case 0: return [];
    case 1: return SIX;
    case 2: return [...model.TOOL_IDS];
    default: {
      const mask = new Rng((g ^ 0x5bd1e995) >>> 0).next() & ALL_TOOLS;
      return model.TOOL_IDS.filter((_: string, i: number) => (mask >>> i) & 1);
    }
  }
}

function startFor(mode: string, s: any, r: any, g: number): void {
  if (mode === 'instant') {
    actions.startRun(s, r, p, {
      start: model.START_EVENT_IDS[g % 5],
      objective: model.OBJECTIVE_IDS[Math.floor(g / 5) % 5],
    });
  } else {
    actions.startRun(s, r, p, { start: model.START_EVENT_IDS[g % 5], tools: toolsFor(g) });
  }
}

const buf = actions.codeBuffer();
function moveFor(mode: string, s: any, r: any): number {
  if (mode === 'instant') return instantCode(s, p);
  const n = actions.legalCodes(s, buf);
  return buf[r.int(n)]!;
}

async function runs(mode: string, from: number, n: number, detail: string): Promise<void> {
  const s = new st.State();
  const MAXREC = 4096;
  const rec = new Int32Array(2 + MAXREC * (1 + SIZE));
  for (let g = from; g < from + n; g++) {
    const r = new Rng(g);
    startFor(mode, s, r, g);
    let k = 0;
    if (detail === 'full') {
      rec[0] = g;
      let o = 2;
      rec[o++] = -1;
      rec.set(s.m, o);
      o += SIZE;
      k = 1;
      while (s.m[st.PHASE] !== st.P_END) {
        const c = moveFor(mode, s, r);
        actions.applyCode(s, c, r, p);
        if (k >= MAXREC) throw new Error(`забег ${g}: больше ${MAXREC} ходов`);
        rec[o++] = c;
        rec.set(s.m, o);
        o += SIZE;
        k++;
      }
      rec[1] = k;
      await writeBin(rec, o);
    } else {
      hReset();
      hPush(-1);
      hState(s.m);
      k = 1;
      while (s.m[st.PHASE] !== st.P_END) {
        const c = moveFor(mode, s, r);
        actions.applyCode(s, c, r, p);
        hPush(c);
        hState(s.m);
        k++;
      }
      await line(`${g} ${k} ${s.m[st.STORAGE]} ${h1 >>> 0} ${h2 >>> 0}`);
    }
  }
  await flushLines();
}

// ── решатель ────────────────────────────────────────────────────────────────

/** Позиции prison-solve-bench: решения мгновенной стратегии в её забегах (сиды 4242+g). */
function solvePositions(games: number, every: number): any[] {
  const out: any[] = [];
  for (let g = 0; g < games; g++) {
    const s = new st.State();
    const r = new Rng(4242 + g);
    actions.startRun(s, r, p, { start: model.START_EVENT_IDS[g % 5] });
    let k = 0;
    while (s.m[st.PHASE] !== st.P_END) {
      if (actions.legalCodes(s, buf) > 1 && k++ % every === 0) out.push(st.clone(s));
      actions.applyCode(s, instantCode(s, p), r, p);
    }
  }
  return out;
}

const hex = (x: number) => {
  const dv = new DataView(new ArrayBuffer(8));
  dv.setFloat64(0, x);
  return dv.getBigUint64(0).toString(16);
};

async function solveCmd(rule: string, from: number, n: number, rounds: number, games: number, every: number): Promise<void> {
  const pos = solvePositions(games, every);
  const free = rule === 'free' ? { minRounds: Infinity, zStop: Infinity, eps: -1 } : {};
  for (let i = from; i < Math.min(from + n, pos.length); i++) {
    solver.clearSolveCache();
    const res = solver.solve(pos[i], { ...free, seed: 1000 + i, maxRounds: rounds }).step(Infinity);
    const opts = res.options.map(
      (o: any) => `${actions.encode(o.action)}:${hex(o.mean)}:${hex(o.gapSe)}:${o.rounds}:${o.pruned ? 1 : 0}`,
    );
    await line(`${i} ${res.rounds} ${res.done ? 1 : 0} ${actions.encode(res.best)} ${opts.length} ${opts.join(' ')}`);
  }
  await flushLines();
}

// ── ГСЧ ─────────────────────────────────────────────────────────────────────

async function rng(from: number, n: number, count: number): Promise<void> {
  for (let g = from; g < from + n; g++) {
    const out: (number | string)[] = [g];
    let r = new Rng(g);
    for (let i = 0; i < count; i++) out.push(r.next());
    r = new Rng(g);
    for (let i = 0; i < 8; i++) out.push(r.bits30());
    // float — точные биты через toString с полной точностью
    for (let i = 0; i < 4; i++) out.push(r.float().toString());
    for (const k of [1, 2, 3, 4, 5, 16, 31, 1000, 0x40000000]) out.push(r.int(k));
    for (const q of [0, 0.25, 0.35, 0.5, 0.8, 1]) out.push(r.chance(q) ? 1 : 0);
    out.push(r.s);
    for (const label of ['doors', 'событие', g & 0xffff]) {
      const t = r.stream(label);
      out.push(t.next(), t.next());
    }
    await line(out.join(' '));
  }
  await flushLines();
}

// ── веса ────────────────────────────────────────────────────────────────────

async function weights(which: string): Promise<void> {
  const dv = new DataView(new ArrayBuffer(8));
  const W = which === 'hand' ? fromJson(HAND) : DEFAULT_WEIGHTS;
  for (let i = 0; i < W.length; i++) {
    dv.setFloat64(0, W[i]!);
    await line(`${i} ${dv.getBigUint64(0).toString(16)}`);
  }
  await flushLines();
}

// ── замеры ──────────────────────────────────────────────────────────────────

function q(sorted: number[], f: number): number {
  return sorted[Math.min(sorted.length - 1, Math.floor(f * sorted.length))]!;
}

async function bench(selfplaySeeds: number): Promise<void> {
  const out = (s: string) => console.log(s);
  // 1. Случайные доигрывания — как src/cli/prison-bench.ts (медиана из 5 серий по 20 000).
  const CASES: [string, string[]][] = [['без орудий', []], ['6 орудий', SIX], ['все 31 орудие', [...model.TOOL_IDS]]];
  const N = 20000;
  const s = new st.State();
  for (const [label, tools] of CASES) {
    const r = new Rng(1);
    const start = new st.State();
    actions.startRun(start, r, p, { tools });
    let total = 0;
    for (let i = 0; i < 5000; i++) {
      st.copy(start, s);
      total += actions.randomPlayout(s, r, p, buf);
    }
    const times: number[] = [];
    for (let k = 0; k < 5; k++) {
      const t0 = performance.now();
      for (let i = 0; i < N; i++) {
        st.copy(start, s);
        total += actions.randomPlayout(s, r, p, buf);
      }
      times.push(((performance.now() - t0) * 1000) / N);
    }
    times.sort((a, b) => a - b);
    out(`random ${label}: ${times[2]!.toFixed(2)} мкс (медиана из 5), ${(total / (5000 + 5 * N)).toFixed(3)} призм.`);
  }

  // 2. Доигрывание мгновенной стратегией (policyPlayout, без проверок): от старта
  //    забегов instant-сидов 0…K−1, медиана из 5 серий.
  {
    const K = 4000;
    const starts: any[] = [];
    for (let g = 0; g < K; g++) {
      const t = new st.State();
      startFor('instant', t, new Rng(g), g);
      starts.push(t);
    }
    const r = new Rng(12345);
    let total = 0;
    for (let i = 0; i < K; i++) {
      st.copy(starts[i], s);
      total += policyPlayout(s, r, p);
    }
    const times: number[] = [];
    for (let k = 0; k < 5; k++) {
      const t0 = performance.now();
      for (let i = 0; i < K; i++) {
        st.copy(starts[i], s);
        total += policyPlayout(s, r, p);
      }
      times.push(((performance.now() - t0) * 1000) / K);
    }
    times.sort((a, b) => a - b);
    out(`policy playout: ${times[2]!.toFixed(2)} мкс (медиана из 5), ${(total / (6 * K)).toFixed(3)} призм.`);
  }

  // 3. Мгновенный совет: позиции из самоигры сидов 0…299; среднее по пакету и
  //    перцентили по отдельным вызовам.
  {
    const pos: any[] = [];
    for (let g = 0; g < 300; g++) {
      const t = new st.State();
      const r = new Rng(g);
      startFor('instant', t, r, g);
      while (t.m[st.PHASE] !== st.P_END) {
        pos.push(st.clone(t));
        actions.applyCode(t, instantCode(t, p), r, p);
      }
    }
    let sink = 0;
    for (let rep = 0; rep < 3; rep++) for (const t of pos) sink += instantCode(t, p);
    const batches: number[] = [];
    for (let rep = 0; rep < 5; rep++) {
      const t0 = performance.now();
      for (const t of pos) sink += instantCode(t, p);
      batches.push(((performance.now() - t0) * 1000) / pos.length);
    }
    batches.sort((a, b) => a - b);
    const each: number[] = [];
    for (const t of pos) {
      const t0 = performance.now();
      sink += instantCode(t, p);
      each.push((performance.now() - t0) * 1000);
    }
    each.sort((a, b) => a - b);
    out(
      `instant advice: ${pos.length} позиций, среднее ${batches[2]!.toFixed(3)} мкс (медиана из 5 пакетов); ` +
        `по вызовам медиана ${q(each, 0.5).toFixed(2)}, p95 ${q(each, 0.95).toFixed(2)}, max ${q(each, 1).toFixed(1)} мкс [${sink & 1}]`,
    );
  }

  // 4. Самоигра мгновенной стратегией: instant-сиды 0…N−1 целиком (startRun +
  //    instantCode + applyCode с проверкой), один поток.
  {
    const t0 = performance.now();
    let total = 0;
    let moves = 0;
    for (let g = 0; g < selfplaySeeds; g++) {
      const r = new Rng(g);
      startFor('instant', s, r, g);
      while (s.m[st.PHASE] !== st.P_END) {
        actions.applyCode(s, instantCode(s, p), r, p);
        moves++;
      }
      total += s.m[st.STORAGE];
    }
    const sec = (performance.now() - t0) / 1000;
    out(
      `selfplay ${selfplaySeeds}: ${sec.toFixed(2)} с, ${((sec * 1e6) / selfplaySeeds).toFixed(2)} мкс на забег, ` +
        `${(moves / selfplaySeeds).toFixed(1)} ходов, ${(total / selfplaySeeds).toFixed(4)} призм. в среднем`,
    );
  }
}

// ── разбор команды ──────────────────────────────────────────────────────────

const num = (i: number, d?: number) => (rest[i] === undefined ? d! : Number(rest[i]));
switch (cmd) {
  case 'weights':
    await weights(rest[0] ?? 'default');
    break;
  case 'rng':
    await rng(num(0), num(1), num(2, 16));
    break;
  case 'runs':
    if (rest[0] !== 'instant' && rest[0] !== 'random') throw new Error('runs instant|random');
    await runs(rest[0], num(1), num(2), rest[3] ?? 'hash');
    break;
  case 'bench':
    await bench(num(0, 200000));
    break;
  case 'solve':
    await solveCmd(rest[0] ?? 'default', num(1), num(2), num(3), num(4, 18), num(5, 2));
    break;
  case 'play': {
    const from = num(0), n = num(1), rounds = num(2);
    for (let g = from; g < from + n; g++) {
      const scores: number[] = [];
      for (const useSolver of [false, true]) {
        const s = new st.State();
        const r = new Rng(777_000 + g);
        actions.startRun(s, r, p, { start: model.START_EVENT_IDS[g % 5] });
        let move = 0;
        if (useSolver) hReset();
        while (s.m[st.PHASE] !== st.P_END) {
          let c: number;
          if (useSolver && actions.legalCodes(s, buf) > 1) {
            solver.clearSolveCache();
            c = actions.encode(solver.solve(s, { seed: g * 1000 + move, maxRounds: rounds }).step(Infinity).best);
          } else c = instantCode(s, p);
          if (useSolver) hPush(c);
          actions.applyCode(s, c, r, p);
          move++;
        }
        scores.push(s.m[st.STORAGE]);
      }
      await line(`${g} ${scores[0]} ${scores[1]} ${h1 >>> 0} ${h2 >>> 0}`);
    }
    await flushLines();
    break;
  }
  case 'scores': {
    const kind = rest[0], seed = num(1), n = num(2);
    const w = kind === 'policy' ? DEFAULT_WEIGHTS : fromJson(HAND);
    const s = new st.State();
    const r = new Rng(1);
    hReset();
    let total = 0;
    for (let i = 0; i < n; i++) {
      r.seed(Math.imul(seed ^ i, 0x9e3779b1) ^ i);
      actions.startRun(s, r, p, { start: model.START_EVENT_IDS[i % 5], startPick: true });
      const x = kind === 'random' ? actions.randomPlayout(s, r, p, buf) : kind === 'greedy' ? greedyPlayout(s, r, p) : policyPlayout(s, r, p, w);
      hPush(x);
      total += x;
    }
    await line(`${n} ${total / n} ${h1 >>> 0} ${h2 >>> 0}`);
    await flushLines();
    break;
  }
  case 'read': {
    const { readFileSync } = await import('node:fs');
    const rec = await imp('core/prison/recognize/index.ts');
    const { decodePng } = await imp('core/prison/recognize/png.ts');
    const mode = rest[0];
    const files = readFileSync(rest[1]!, 'utf8').split(/\r?\n/).filter(Boolean);
    let geo: any = null;
    for (const f of files) {
      const frame = decodePng(readFileSync(f));
      const r = rec.read(frame, mode === 'fresh' ? null : geo, { recalibrate: mode === 'chainre' });
      geo = r.geo;
      await line(JSON.stringify({ file: f, view: r.view, geo: r.geo }));
    }
    await flushLines();
    break;
  }
  case 'selfplay': {
    const s = new st.State();
    let total = 0, moves = 0;
    const from = num(0), n = num(1);
    for (let g = from; g < from + n; g++) {
      const r = new Rng(g);
      startFor('instant', s, r, g);
      while (s.m[st.PHASE] !== st.P_END) {
        actions.applyCode(s, instantCode(s, p), r, p);
        moves++;
      }
      total += s.m[st.STORAGE];
    }
    console.log(`${n} ${moves} ${total}`);
    break;
  }
  default:
    console.error(`неизвестная команда ${cmd}`);
    process.exit(2);
}
