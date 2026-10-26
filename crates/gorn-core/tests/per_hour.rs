//! «В час» — сумма за забеги ÷ их общее время, а не медиана темпов.

use gorn_core::analysis::{rolling_per_hour, session_clock};
use gorn_core::stats::summarize_timed;

const MIN: i64 = 60_000;

#[test]
fn per_hour_is_total_over_time() {
    // Три карты по 10 минут: 100, 100 и 1000 ФЕ — за полчаса 1200, то есть 2400 в час.
    let nets = [100.0, 100.0, 1000.0];
    let actives = [10 * MIN, 10 * MIN, 10 * MIN];
    let rates: Vec<f64> = nets.iter().map(|n| n * 6.0).collect();
    let s = summarize_timed(&rates, Some(&nets), nets.iter().sum(), actives.iter().sum());
    assert!((s.per_hour - 2400.0).abs() < 1e-9);
    // Медиана той же выборки удачную карту не видит.
    assert!((s.median - 600.0).abs() < 1e-9);
}

#[test]
fn per_hour_weights_runs_by_time() {
    // Короткая карта с высоким темпом не перевешивает длинную.
    let nets = [50.0, 300.0];
    let actives = [MIN, 30 * MIN];
    let rates: Vec<f64> = nets.iter().zip(&actives).map(|(n, a)| n / *a as f64 * 3_600_000.0).collect();
    let s = summarize_timed(&rates, None, 350.0, 31 * MIN);
    assert!((s.per_hour - 350.0 / 31.0 * 60.0).abs() < 1e-9);
}

#[test]
fn rolling_per_hour_takes_last_window() {
    let nets = [1000.0, 60.0, 30.0];
    let actives = [MIN, 30 * MIN, 30 * MIN];
    // Окно 2: последние два забега — 90 ФЕ за час.
    assert!((rolling_per_hour(&nets, &actives, 2) - 90.0).abs() < 1e-9);
    assert_eq!(rolling_per_hour(&[], &[], 20), 0.0);
}

#[test]
fn session_clock_is_wall_time() {
    // Игра пишет лог — время идёт до «сейчас», город и перерывы тоже.
    assert_eq!(session_clock(0, 94 * MIN - 5_000, 94 * MIN), 94 * MIN);
    // Игру закрыли 30 минут назад — часы встали на её последней строке.
    assert_eq!(session_clock(0, 60 * MIN, 90 * MIN), 60 * MIN);
    // Лога ещё не видели — времени нет.
    assert_eq!(session_clock(0, 0, 10 * MIN), 0);
}
