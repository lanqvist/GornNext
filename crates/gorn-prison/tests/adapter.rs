//! Память забега: JSON того же вида, что у эталона, читается обратно без потерь;
//! испорченное сохранение — чистый забег.

use gorn_prison::adapter::{blank_memory, load_memory, Cause, DecisionMem, PrisonAdapter, Step};
use gorn_prison::model::{start_event, tool, CELLS};
use gorn_prison::view::{BoardAt, BoardView, CellKind, CellView};
use serde_json::json;

#[test]
fn memory_roundtrip() {
    let mut a = PrisonAdapter::new(None);
    a.mem.day = 7;
    a.mem.tools = vec![1, 17];
    a.mem.step = Step::Manage;
    a.mem.storage = 3;
    a.mem.objective = Some(2);
    a.mem.decision = Some(DecisionMem { consume: true, replace: false, end: true, hunt: false, after_chain: true });
    let j = a.mem.to_json();
    assert_eq!(load_memory(&j), a.mem);
    assert_eq!(j["tools"], json!(["keys", "maul"]));
    assert_eq!(j["step"], json!("manage"));
}

#[test]
fn broken_memory_is_blank() {
    assert_eq!(load_memory(&json!({ "v": 2 })), blank_memory());
    assert_eq!(load_memory(&json!(null)), blank_memory());
    assert_eq!(load_memory(&json!({ "v": 1, "cells": [] })), blank_memory());
}

/// Журнал фактов 24.10 10:07: Горн открыт посреди нового забега — день по логу
/// ушёл с 19 на 0, память прошлого (хранилище, орудия) сбрасывается.
#[test]
fn day_going_back_starts_new_run() {
    let mut a = PrisonAdapter::new(None);
    a.log_day(Some((20, 20)));
    a.mem.storage = 8;
    a.mem.storage_floor = 8;
    a.mem.tools = vec![11, 20];
    a.mem.objective_done = true;
    a.log_day(Some((1, 20)));
    assert_eq!((a.mem.day, a.mem.storage, a.mem.storage_floor), (0, 0, 0));
    assert!(a.mem.tools.is_empty());
    assert!(!a.mem.objective_done && !a.mem.run_start);
    // Соседний день и тот же — не сброс.
    a.mem.storage = 2;
    a.log_day(Some((5, 20)));
    a.log_day(Some((4, 20)));
    assert_eq!((a.mem.day, a.mem.storage), (3, 2));
}

/// Когти ставят «Царапину» и в первое утро, сразу после выбора: выбор орудий
/// («день 0») и первая доска («день 1») оба дают день 0 (журнал фактов #43,
/// 25.10 11:51: синяя царапина на №13 первой доски).
#[test]
fn claws_scratch_on_first_board() {
    let mut a = PrisonAdapter::new(None);
    a.start_run();
    a.log_day(Some((1, 20)));
    a.mem.tools = vec![tool::CLAWS];
    let mut cells: Vec<CellView> = (0..CELLS).map(|_| CellView::new(CellKind::Locked)).collect();
    cells[0] = CellView::new(CellKind::Empty);
    cells[12].shown_tier = Some(1);
    let at = BoardAt { cells: vec![], door_cards: vec![], decision_cards: vec![], decision_kinds: vec![], cancel: None };
    let v = BoardView {
        cells,
        warden: Some(0),
        days_left: None,
        doors: vec![],
        decision: None,
        chain: None,
        tools: vec![],
        tool_slots: None,
        chromatic: -1,
        objective: None,
        at,
    };
    a.board(&v);
    assert!(a.mem.cells[12].scratch && a.mem.cells[12].shown == Some(1));
    // Та же доска позже в этот день — не утро: новых царапин нет.
    let mut later = v.clone();
    later.cells[5].shown_tier = Some(0);
    a.board(&later);
    assert!(a.mem.cells[12].scratch && !a.mem.cells[5].scratch);
}

/// «Расточительность»: за каждого нового Призматического в цепочке — по 3
/// выбора (журнал фактов 24.10 12:24–12:25: хранилище 3 → 7 за окном подарков).
#[test]
fn abundance_gives_gifts_per_prism() {
    let mut a = PrisonAdapter::new(None);
    a.chain_gifts = true;
    a.mem.start = start_event::ABUNDANCE;
    a.log_day(Some((13, 20)));
    a.mem.storage = 3;
    a.treasure(false, Cause::Consume);
    assert_eq!(a.mem.gifts, 3);
    // Призма укуса уже за окном: 3 выбора, один взят.
    a.mem.storage = 4;
    a.treasure(true, Cause::None);
    assert_eq!(a.mem.gifts, 3);
    a.gift_taken();
    assert_eq!(a.mem.gifts, 2);
    // Подарок дал ещё призму — ещё 3 выбора.
    a.mem.storage = 5;
    a.treasure(true, Cause::None);
    assert_eq!(a.mem.gifts, 5);
}

/// «Бивень» Щипцов: значок на портрете слона (забег #52, 25.10 20:21) — в памяти
/// камеры и во флагах решателя; переживает Замену и тёмный кадр, сходит с пустой
/// камерой.
#[test]
fn tongs_tusk_mark() {
    use gorn_prison::state::{FLAGS, F_TUSK};
    let mut a = PrisonAdapter::new(None);
    a.start_run();
    a.log_day(Some((7, 20)));
    a.mem.tools = vec![tool::TONGS];
    let mut cells: Vec<CellView> = (0..CELLS).map(|_| CellView::new(CellKind::Locked)).collect();
    cells[0] = CellView::new(CellKind::Empty);
    for (n, b, tusk) in [(5, 0, true), (6, 0, false), (1, 2, false)] {
        cells[n] = CellView::new(CellKind::Inmate);
        cells[n].beast = Some(b);
        cells[n].tier = Some(0);
        cells[n].tusk = Some(tusk);
    }
    let at = BoardAt { cells: vec![], door_cards: vec![], decision_cards: vec![], decision_kinds: vec![], cancel: None };
    let v = BoardView {
        cells,
        warden: Some(0),
        days_left: None,
        doors: vec![],
        decision: None,
        chain: None,
        tools: vec![],
        tool_slots: None,
        chromatic: -1,
        objective: None,
        at,
    };
    a.board(&v);
    assert!(a.mem.cells[5].tusk && !a.mem.cells[6].tusk);
    assert_eq!(a.build().m[FLAGS + 5] & F_TUSK, F_TUSK);
    assert_eq!(a.build().m[FLAGS + 6] & F_TUSK, 0);
    assert_eq!(load_memory(&a.mem.to_json()), a.mem);

    // Замена: в камере волк, значок на тёмном кадре не узнан — метка остаётся.
    let mut swapped = v.clone();
    swapped.cells[5].beast = Some(2);
    swapped.cells[5].tusk = Some(false);
    a.board(&swapped);
    assert!(a.mem.cells[5].tusk);

    // Без Щипцов метка решателю ни к чему.
    a.mem.tools = vec![];
    assert_eq!(a.build().m[FLAGS + 5] & F_TUSK, 0);
    a.mem.tools = vec![tool::TONGS];

    // Узника съели — камера заперта, потом открыта другим: метки нет.
    let mut eaten = swapped.clone();
    eaten.cells[5] = CellView::new(CellKind::Locked);
    a.board(&eaten);
    a.board(&swapped);
    assert!(!a.mem.cells[5].tusk);
}

/// «Выберите цель» по нажатию карточки решения у льва с «Капканом» — добыча, даже
/// если ряда с «Выслеживанием» не видели [кадры 26.10 23:50:53–54]; после «Подтвердить»
/// окна события — крысиная тропа.
#[test]
fn target_screen_after_action_click_is_hunt() {
    use gorn_prison::view::Decision;
    let board = |decision: Option<Decision>| {
        let mut cells: Vec<CellView> = (0..CELLS).map(|_| CellView::new(CellKind::Locked)).collect();
        cells[0] = CellView::new(CellKind::Office);
        for (n, b, t) in [(3, 1, 4), (15, 2, 4), (10, 3, 4)] {
            cells[n] = CellView::new(CellKind::Inmate);
            cells[n].beast = Some(b);
            cells[n].tier = Some(t);
        }
        BoardView {
            cells,
            warden: Some(3),
            days_left: Some(7),
            doors: vec![],
            decision,
            chain: None,
            tools: vec![],
            tool_slots: None,
            chromatic: -1,
            objective: None,
            at: BoardAt { cells: vec![], door_cards: vec![], decision_cards: vec![], decision_kinds: vec![], cancel: None },
        }
    };
    for (click, trap, want) in [(true, true, Step::Hunt), (false, true, Step::Ratway), (true, false, Step::Ratway)] {
        let mut a = PrisonAdapter::new(None);
        a.start_run();
        a.log_day(Some((13, 20)));
        a.mem.tools = if trap { vec![tool::TRAP] } else { vec![] };
        // Утренний обход уже был: «Выберите цель» в начале дня — вчерашняя надпись.
        let mut patrol = board(None);
        patrol.doors = vec![3];
        a.board(&patrol);
        a.action_click = click;
        a.board(&board(Some(Decision::Ratway)));
        assert_eq!(a.mem.step, want, "нажатие {click}, капкан {trap}");
    }
}

/// Царапина Когтей: «?» за дверью гаснет под окном подарков (память держит),
/// а на яркой доске его нет — узника съели сквозь дверь, царапины больше нет
/// [кадры 27.10 11:15:43–11:16:01: «съесть №12» в уже съеденную дверь].
#[test]
fn claws_scratch_gone_on_bright_board() {
    use gorn_prison::view::Decision;
    let mut a = PrisonAdapter::new(None);
    a.start_run();
    a.log_day(Some((5, 20)));
    a.mem.tools = vec![tool::CLAWS];
    let mut cells: Vec<CellView> = (0..CELLS).map(|_| CellView::new(CellKind::Locked)).collect();
    cells[0] = CellView::new(CellKind::Office);
    cells[3] = CellView::new(CellKind::Inmate);
    cells[3].beast = Some(3);
    cells[3].tier = Some(1);
    cells[11].shown_tier = Some(1);
    let at = BoardAt { cells: vec![], door_cards: vec![], decision_cards: vec![], decision_kinds: vec![], cancel: None };
    let patrol = BoardView {
        cells,
        warden: Some(3),
        days_left: None,
        doors: vec![4],
        decision: None,
        chain: None,
        tools: vec![],
        tool_slots: None,
        chromatic: -1,
        objective: None,
        at,
    };
    a.board(&patrol);
    assert!(a.mem.cells[11].scratch && a.mem.cells[11].shown == Some(1));
    // Окно подарков поверх доски: «?» не видно — память держит.
    let mut overlay = patrol.clone();
    overlay.doors = vec![];
    overlay.cells[11].shown_tier = None;
    a.board(&overlay);
    assert!(a.mem.cells[11].scratch && a.mem.cells[11].shown == Some(1));
    // Ряд решений: «?» нет — двери-цели больше нет.
    let mut manage = overlay.clone();
    manage.decision = Some(Decision::Manage { consume: true, replace: false, end: true, hunt: false });
    a.board(&manage);
    assert!(!a.mem.cells[11].scratch && a.mem.cells[11].shown.is_none());
}

/// «Цепная реакция» (кадры 27.10 10:06:21–26): окно снято, пока в памяти
/// надзиратель ещё в допросной; ряд решений в камере 1 — реакция ждёт, утренние
/// двери из 1 — совет знает о ней; ушёл в 6 — отработала.
#[test]
fn chain_reaction_waits_for_next_patrol() {
    use gorn_prison::adapter::Position;
    use gorn_prison::state::CHAIN_REACTION;
    use gorn_prison::view::Decision;
    let mut a = PrisonAdapter::new(None);
    a.start_run();
    a.log_day(Some((1, 20)));
    let mut cells: Vec<CellView> = (0..CELLS).map(|_| CellView::new(CellKind::Locked)).collect();
    cells[0] = CellView::new(CellKind::Office);
    cells[1] = CellView::new(CellKind::Empty);
    let at = BoardAt { cells: vec![], door_cards: vec![], decision_cards: vec![], decision_kinds: vec![], cancel: None };
    let manage = BoardView {
        cells,
        warden: Some(1),
        days_left: None,
        doors: vec![],
        decision: Some(Decision::Manage { consume: false, replace: false, end: true, hunt: false }),
        chain: None,
        tools: vec![],
        tool_slots: None,
        chromatic: -1,
        objective: None,
        at,
    };
    assert_eq!(a.mem.warden, 0);
    a.chain_reaction();
    a.board(&manage);
    assert_eq!(a.mem.chain_reaction, Some(1));
    let mut patrol = manage.clone();
    patrol.decision = None;
    patrol.doors = vec![3, 5, 6];
    a.log_day(Some((2, 20)));
    a.board(&patrol);
    assert_eq!(a.mem.chain_reaction, Some(1));
    assert_eq!(load_memory(&a.mem.to_json()), a.mem);
    match a.position() {
        Position::Ok { state, .. } => assert_eq!(state.m[CHAIN_REACTION], 1),
        Position::No { problem, .. } => panic!("{problem}"),
    }
    let mut moved = patrol.clone();
    moved.cells[6] = CellView::new(CellKind::Empty);
    moved.warden = Some(6);
    moved.doors = vec![];
    moved.decision = Some(Decision::Manage { consume: false, replace: false, end: true, hunt: false });
    a.board(&moved);
    assert_eq!(a.mem.chain_reaction, None);
}

/// Прогресс цели с панели: в памяти только растёт, в состоянии решателя — не
/// больше цели − 1, после выполнения цели не передаётся.
#[test]
fn objective_progress_goes_to_solver() {
    use gorn_prison::model::objective;
    use gorn_prison::state::{OBJECTIVE_ID, OBJ_PROGRESS};
    let mut a = PrisonAdapter::new(None);
    a.start_run();
    a.log_day(Some((3, 20)));
    a.mem.objective = Some(objective::DOORS);
    a.mem.step = Step::Patrol;
    a.objective_progress(7);
    a.objective_progress(1);
    assert_eq!(a.mem.objective_progress, 7);
    assert_eq!(load_memory(&a.mem.to_json()), a.mem);
    let st = a.build();
    assert_eq!((st.m[OBJECTIVE_ID], st.m[OBJ_PROGRESS]), (objective::DOORS, 7));
    a.objective_progress(30);
    assert_eq!(a.build().m[OBJ_PROGRESS], 9);
    a.mem.objective_done = true;
    assert_eq!((a.build().m[OBJECTIVE_ID], a.build().m[OBJ_PROGRESS]), (-1, 0));
    a.start_run();
    assert_eq!(a.mem.objective_progress, 0);
}
