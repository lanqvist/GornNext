//! «В час» — сумма за забеги ÷ их общее время, а не медиана темпов.

use gorn_core::analysis::{rolling_per_hour, session_time};
use gorn_core::stats::summarize_timed;
use gorn_core::Run;

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

fn segment(started_at: i64, ended_at: Option<i64>, is_hub: bool) -> Run {
    Run {
        area_id: 1,
        asset: None,
        level_type: None,
        started_at,
        ended_at,
        duration_ms: ended_at.map(|e| e - started_at),
        is_hub,
        player_id: None,
        season_id: None,
        drops: vec![],
        costs: vec![],
    }
}

#[test]
fn session_time_counts_town_up_to_a_break() {
    // Город 2 минуты — целиком; 40 минут (ушёл) — только 5.
    assert_eq!(session_time(&segment(0, Some(2 * MIN), true), 0, None), 2 * MIN);
    assert_eq!(session_time(&segment(0, Some(40 * MIN), true), 0, None), 5 * MIN);
    // Город, начатый до сессии, — только его часть после начала сессии.
    assert_eq!(session_time(&segment(0, Some(3 * MIN), true), 2 * MIN, None), MIN);
    // Открытый отрезок — до «сейчас».
    assert_eq!(session_time(&segment(0, None, true), 0, Some(MIN)), MIN);
    // Карта: 4 минуты без событий — меньше 5-минутного перерыва, идут целиком.
    assert_eq!(session_time(&segment(0, Some(4 * MIN), false), 0, None), 4 * MIN);
}
