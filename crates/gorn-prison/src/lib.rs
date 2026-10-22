//! Тюрьма Талоса: движок правил, ГСЧ и мгновенная стратегия.
//!
//! Перенос ядра тюрьмы Горна (gorn-tli, `src/core/prison`) бит в бит:
//! тот же сид даёт тот же забег. Правила — docs/prison/rules.md эталона;
//! при расхождении текста и кода верен код эталона.

pub mod actions;
pub mod adapter;
pub mod advisor;
pub mod hint;
pub mod logevents;
pub mod model;
pub mod policy;
pub mod recognize;
pub mod rng;
pub mod rules;
pub mod runs;
pub mod solver;
pub mod state;
pub mod tables;
pub mod trace;
pub mod view;

pub use actions::{apply_code, apply_trusted, legal_codes, random_playout, start_run, CodeBuf, RunOptions};
pub use model::{Params, DEFAULT_PARAMS};
pub use policy::{default_weights, Instant, Weights};
pub use rng::Rng;
pub use state::{State, StreamHash};
