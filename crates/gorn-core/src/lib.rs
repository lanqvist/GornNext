//! Трекер Горна: разбор лога игры, сумка, забеги, биржа, цены, анализ.
//!
//! Перенос ядра трекера gorn-tli (`src/core`) с той же логикой: реальные
//! логи дают тот же разбор, что у эталона (`gorn-cli verify --logs`).

pub mod inventory;
pub mod parser;
pub mod patterns;
pub mod pipeline;
pub mod segmenter;
pub mod trade;
pub mod types;

pub use pipeline::{analyze_bytes, analyze_log_file, AnalysisResult, AnalyzeOptions};
pub use types::*;
