//! Забеги по сиду — общие для самоигры, замеров и сверки с эталоном.
//! Определения те же, что в tools/reference.mts (TS): сид g однозначно
//! задаёт тему, цель, орудия и все кости забега.
//!
//! - `Instant`: Rng(g), тема g % 5, цель ⌊g / 5⌋ % 5, выбор орудия в начале,
//!   ходы мгновенной стратегии, применение с проверкой;
//! - `Random`: Rng(g), тема g % 5, цель случайная (бросок движка), орудия по
//!   g % 4: нет / 6 из prison-bench / все 31 / маска из Rng(g ^ 0x5bd1e995);
//!   ходы — случайные легальные, как в `random_playout`.

use crate::actions::{apply_code, legal_codes, start_run, CodeBuf, RunOptions};
use crate::model::{tool, Params};
use crate::policy::{Instant, Weights};
use crate::rng::Rng;
use crate::state::{State, StreamHash, PHASE, P_END, STORAGE};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Instant,
    Random,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::Instant => "instant",
            Mode::Random => "random",
        }
    }

    pub fn parse(s: &str) -> Option<Mode> {
        match s {
            "instant" => Some(Mode::Instant),
            "random" => Some(Mode::Random),
            _ => None,
        }
    }
}

/// Шесть орудий из prison-bench: ключи, монета, капкан, когти, цепь, весы.
pub const SIX_TOOLS: i32 = (1 << tool::KEYS)
    | (1 << tool::COIN)
    | (1 << tool::TRAP)
    | (1 << tool::CLAWS)
    | (1 << tool::RAVENOUS)
    | (1 << tool::SCALES);
pub const ALL_TOOLS: i32 = 0x7fff_ffff;

/// Орудия забега `Random` для сида g.
pub fn tools_for(g: u32) -> i32 {
    match g % 4 {
        0 => 0,
        1 => SIX_TOOLS,
        2 => ALL_TOOLS,
        _ => (Rng::new(g ^ 0x5bd1_e995).next() as i32) & ALL_TOOLS,
    }
}

/// Начать забег сида g.
pub fn start_for(mode: Mode, s: &mut State, r: &mut Rng, p: &Params, g: u32) {
    let opts = match mode {
        Mode::Instant => RunOptions {
            start: (g % 5) as i32,
            objective: Some(((g / 5) % 5) as i32),
            ..RunOptions::default()
        },
        Mode::Random => RunOptions { start: (g % 5) as i32, tools: tools_for(g), ..RunOptions::default() },
    };
    start_run(s, r, p, opts);
}

/// Итог забега для сверки по хешу.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Summary {
    /// Записей: состояние после старта и после каждого хода.
    pub steps: u32,
    pub score: i32,
    pub h1: u32,
    pub h2: u32,
}

/// Игрок забегов: рабочие таблицы стратегии и буфер ходов (по одному на поток).
pub struct Runner<'a> {
    pub p: &'a Params,
    pub w: &'a Weights,
    pub inst: Instant,
    buf: CodeBuf,
}

impl<'a> Runner<'a> {
    pub fn new(p: &'a Params, w: &'a Weights) -> Self {
        Runner { p, w, inst: Instant::new(), buf: [0; 32] }
    }

    #[inline]
    fn move_for(&mut self, mode: Mode, s: &State, r: &mut Rng) -> i32 {
        match mode {
            Mode::Instant => self.inst.code(s, self.p, self.w),
            Mode::Random => {
                let n = legal_codes(s, &mut self.buf);
                self.buf[r.int(n as i32) as usize]
            }
        }
    }

    /// Сыграть забег; `on` получает (код, состояние) после старта (код −1) и после каждого хода.
    pub fn run_with(&mut self, mode: Mode, g: u32, mut on: impl FnMut(i32, &State)) -> i32 {
        let mut s = State::new();
        let mut r = Rng::new(g);
        start_for(mode, &mut s, &mut r, self.p, g);
        on(-1, &s);
        while s.m[PHASE] != P_END {
            let c = self.move_for(mode, &s, &mut r);
            if let Err(e) = apply_code(&mut s, c, &mut r, self.p) {
                panic!("сид {g}: {e}");
            }
            on(c, &s);
        }
        s.m[STORAGE]
    }

    /// Сыграть забег и посчитать потоковый хеш (как `runs … hash` в reference.mts).
    pub fn run_hash(&mut self, mode: Mode, g: u32) -> Summary {
        let mut h = StreamHash::new();
        let mut steps = 0;
        let score = self.run_with(mode, g, |c, s| {
            h.push(c);
            h.push_state(s);
            steps += 1;
        });
        Summary { steps, score, h1: h.h1, h2: h.h2 }
    }

    /// Сыграть забег без записи — для замеров самоигры.
    pub fn run(&mut self, mode: Mode, g: u32) -> i32 {
        self.run_with(mode, g, |_, _| {})
    }
}
