//! Стратегии тюрьмы: мгновенная и её веса.

pub mod instant;
pub mod weights;

pub use instant::Instant;
pub use weights::{default_weights, Weights};
