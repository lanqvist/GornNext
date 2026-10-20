//! Трекер Горна: разбор лога игры, сумка, забеги, биржа, цены, анализ.
//!
//! Перенос ядра трекера gorn-tli (`src/core`) с той же логикой: реальные
//! логи дают тот же разбор, что у эталона (`gorn-cli verify --logs`).

pub mod analysis;
pub mod community;
pub mod csv;
pub mod engine;
pub mod facts;
pub mod bag;
pub mod history;
pub mod inventory;
pub mod labels;
pub mod locate;
pub mod mapnames;
pub mod parser;
pub mod patterns;
pub mod pipeline;
pub mod prices;
pub mod retroscan;
pub mod segmenter;
pub mod stats;
pub mod storage;
pub mod tailer;
pub mod trade;
pub mod types;

pub use pipeline::{analyze_bytes, analyze_log_file, AnalysisResult, AnalyzeOptions};
pub use types::*;
