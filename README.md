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
- `crates/gorn-vorax` — Vorax («Хирургическая подготовка»): движок, бот
  и решатель, перенос крейта `vorax` из
  [DinoRaos/vorax-solver-rs](https://github.com/DinoRaos/vorax-solver-rs)
  (MIT, коммит 9aabc20); тесты сверяют его с записанными трассами эталона
  шаг в шаг. Авторские права DinoRaos, tidewarden и Moebing — в
  `crates/gorn-vorax/LICENSE`. Как и у них, только советы: игра не
  управляется;
- `crates/gorn-vorax-scan` — чтение экрана Vorax (перенос `vorax-scan`
  оттуда же, MIT). Своё — русский клиент: карты по картинке, названия по
  OCR заголовка и `data/names-ru.json` (`scan_data_ru`). Тесты на кадрах
  эталона — `VORAX_FIXTURES=<fixtures эталона> cargo test -- --ignored`;
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

## Приложение на Tauri 2

`app/` — `gorn.exe`: трекер валюты (движок `gorn-core`, интерфейс React
из `ui/` — перенос интерфейса Горна) и подсказки в сезонной тюрьме
(советник `gorn-prison`, кадр экрана через Windows.Graphics.Capture, стрелки
поверх игры).

```
cd ui && npm install && npm run build && cd ..   # интерфейс → app/dist
cargo build --release -p gorn-app
target/release/gorn.exe
```

- Данные — в `%APPDATA%\com.gorn.next`. При первом запуске база, настройки
  трекера и кэш цен сообщества копируются из Горна на Electron
  (`%APPDATA%\gorn-tli`); сам он не трогается.
- Мост `window.gorn` (`ui/src/renderer/bridge.ts`) — одна команда `gorn`
  с каналами, как у ipcMain Горна, и события Tauri с теми же именами.
- `gorn.exe --smoke <файл>` — проверка без окна поверх игры: главное окно
  скрыто, подсказки выключены на этот запуск, каждый срез пишется в файл.
- В Горне на Electron выключить «Подсказки в тюрьме»: иначе стрелки двойные
  и Ctrl+Alt+P занят.
- Ctrl+Alt+P — подсказка по экрану сейчас (или пункт в трее).
- Vorax («Хирургическая подготовка»): на каждой новой доске обведены карта
  (или кнопка обновления) и баки, сверху плашка с ходом и долей симуляций,
  доходящих до 840k (`app/src/vorax.rs`, окно `ui/public/vorax.js`).
  Выключается в трее («Подсказки в Vorax»); уровень Интерна Шелли —
  `voraxShelley` в settings.json (по умолчанию 4). Кадры — в «Изображения/
  Горн Next — vorax», что прочитано не наверняка — ещё и в «незнакомое»:
  по ним дополняются эталоны. Проверка без игры:
  `gorn.exe --vorax-read отчёт.txt кадр.png…`.
- Кадры сохраняются в «Изображения/Горн Next — тюрьма» (тот же формат,
  что у Горна): разбор — `gorn-cli audit --shots "<эта папка>"`,
  `gorn-cli replay "<папка дня>"`. Выключается в трее.

`verify` и `bench --ts` ищут эталон в `../gorn-tli` (или `--tli ПУТЬ`),
нужны node и `npm install` в эталоне.

Неофициальный инструмент. Torchlight: Infinite © XD Inc.
