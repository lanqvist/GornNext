//! Запись случайных бросков движка — для подгонки шансов по живым забегам
//! (этап 8, `gorn-cli fit`). Включается в своём потоке на время одного
//! розыгрыша; выключенная стоит одной проверки флага на бросок, ГСЧ не трогает.

use std::cell::{Cell, RefCell};

/// Что разыгрывалось.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum What {
    /// Редкость узника за дверью (`door_odds`).
    Door,
    /// Редкость узника на замену (`replace_odds`).
    Replace,
    /// Шанс по имени поля `Params` (`saw_chance` и т. п.).
    Chance(&'static str),
}

/// Бросок: что, в какой камере (−1 — не к камере), что выпало, нижняя граница редкости.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Draw {
    pub what: What,
    pub cell: i32,
    pub value: i32,
    pub floor: i32,
}

thread_local! {
    static ON: Cell<bool> = const { Cell::new(false) };
    static LOG: RefCell<Vec<Draw>> = const { RefCell::new(Vec::new()) };
}

/// Запись включена в этом потоке.
#[inline]
pub fn on() -> bool {
    ON.with(Cell::get)
}

#[inline]
pub fn record(d: Draw) {
    if on() {
        LOG.with(|l| l.borrow_mut().push(d));
    }
}

/// Начать запись с чистого листа.
pub fn start() {
    LOG.with(|l| l.borrow_mut().clear());
    ON.with(|c| c.set(true));
}

/// Закончить запись и забрать броски.
pub fn take() -> Vec<Draw> {
    ON.with(|c| c.set(false));
    LOG.with(|l| std::mem::take(&mut *l.borrow_mut()))
}
