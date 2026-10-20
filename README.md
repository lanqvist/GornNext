# GornNext

Горн нового поколения — трекер валюты и фарм-эффективности и советник
тюрьмы для Torchlight: Infinite, на Rust и Tauri 2.

Нынешний Горн на Electron — [lanqvist/gorn-tli](https://github.com/lanqvist/gorn-tli).
Он выпускается до переходного релиза и служит эталоном: новая реализация
сверяется с его ядрами на тех же входах.

План перехода, этапы и проверки — [docs/plan.md](docs/plan.md).
Итоги этапа 2 (перенос ядра тюрьмы и замер) — [docs/stage2-results.md](docs/stage2-results.md).

## Устройство

Cargo workspace:

- `crates/gorn-prison` — тюрьма: ГСЧ, состояние, правила, ходы,
  доигрывание и мгновенная стратегия (веса — `data/weights.json`);
- `crates/gorn-cli` — `bench`, `selfplay`, `verify`;
- `tools/reference.mts` — эталон: тот же движок на TS из соседнего
  `../gorn-tli` (через его tsx, сам gorn-tli не меняется).

```
cargo build --release
cargo test
target/release/gorn-cli verify            # сверка с эталоном: 10⁶ сидов
target/release/gorn-cli verify --quick    # быстро: 20 000 сидов
target/release/gorn-cli selfplay --seeds 200000 --threads 0
target/release/gorn-cli bench --ts        # замеры Rust, затем те же в TS
```

`verify` и `bench --ts` ищут эталон в `../gorn-tli` (или `--tli ПУТЬ`),
нужны node и `npm install` в эталоне.

Неофициальный инструмент. Torchlight: Infinite © XD Inc.
