//! Vorax («Хирургическая подготовка»): движок, бот и решатель.
//!
//! Перенос крейта `vorax` из DinoRaos/vorax-solver-rs (MIT, коммит 9aabc20, v0.2.2) без изменений в логике:
//! поменяны только пути к данным и имя крейта. Тесты `tests/parity.rs` проигрывают записанные с эталона трассы
//! (`tests/data/`) и должны совпадать шаг в шаг; при переносе новых версий сверять с ними же.
//! Авторские права — в LICENSE этого крейта.
//!
//! Rust port of tidewarden/tli-vorax-simulator's engine, bot and advisor.
//!
//! Read-only advice: like upstream, nothing built on this crate may play the real game (no clicks, key presses
//! or other input automation acting on its advice). See AGENTS.md upstream.

pub mod advisor;
pub mod bot;
pub mod context;
pub mod data;
pub mod effects;
pub mod game;
pub mod rng;
pub mod stack;
pub mod types;
pub mod value;

pub use data::{data, Data};
pub use types::*;
