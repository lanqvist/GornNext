# Этап 3: тюрьма целиком на Rust — итоги

2026-10-20. Эталон — gorn-tli 83bb91c (1.0.32), `src/core/prison`,
`src/main/prison/ownAdvisor.ts`, `src/cli/prison-*.ts`.

## Что перенесено

| Эталон (gorn-tli) | GornNext |
|---|---|
| `solver/solve.ts` (+ схема пула `pool.ts`, beaccdd) | `crates/gorn-prison/src/solver.rs` — Монте-Карло с общими костями, остановка и отсечение, парная SE, правило смены хода, кэш поисков; пул — rayon |
| `recognize/**` (+ `templates/*.json`) | `src/recognize/` — выборка, нормировка, эталоны (`data/templates`), калибровка и перекалибровка, доска, карточки, цифры, панели, выбор и сокровищница |
| `view.ts` | `src/view.rs` — вид экрана с тем же JSON |
| `adapter.ts` | `src/adapter.rs` — память забега (тот же JSON: сохранённое состояние Горна читается), позиция для совета |
| `hint.ts`, подписи `advise()` | `src/hint.rs`, `Instant::advise` |
| `main/prison/ownAdvisor.ts` | `src/advisor.rs` — без потоков Electron: кадр → ответы |
| `policy/greedy.ts` | `src/policy/greedy.rs` |
| `cli/prison-solve-bench.ts` | `gorn-cli solve-bench` (`--rate`, `--regret`) и `gorn-cli play` (`--play`) |
| `cli/prison-tune.ts` | `gorn-cli tune` (`--report`) |
| `cli/prison-replay.ts`, `prison-moves.ts` | `gorn-cli replay` |
| `cli/prison-audit.ts`, `prison-audit-core.ts` | `gorn-cli audit` (с находкой «петля») |

Правка 1.0.32 «Рог не засчитывает скачок Молота» (0201290) перенесена в
`policy/instant.rs`: эталон обновился посреди этапа, сверка её поймала
(одна партия из 64 в решателе на кадре 20.10 00:09:17 — Рог против Вина при
Молоте).

## Сверка

`gorn-cli verify` — всё против `tools/reference.mts` (эталон на TS в
16 процессах node).

| Проверка | Объём | Итог |
|---|---|---|
| движок и мгновенная стратегия (этап 2): забеги по хешу и целиком | … | … |
| решатель при фиксированном числе раундов: оценки, SE, отсечение, остановка | 580 позиций аудита × 2 правила (по умолчанию и без остановки), 400 раундов | … |
| пул rayon = один поток | те же позиции | … |
| самоигра с решателем без срока (ход в ход) | … забегов, 300 раундов | … |
| забеги настройки (случайные, жадная, стратегия) | … | … |
| распознавание: вид экрана и геометрия, fresh / chain / chainre | 996 кадров (все PNG `gorn-prison-data/images`) | … |
| советник на сохранённых кадрах: ответы, память забега, оценки ходов | … кадров, 4 дня `gorn-prison-data/runs`, 64 раунда | … |
| ядро аудита: разбивка дней, находки, сводка | … | … |

## Замеры

…

## Что дальше

…
