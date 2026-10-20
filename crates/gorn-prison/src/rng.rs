//! ГСЧ движка тюрьмы: одно 32-битное состояние (mulberry32), детерминированный.
//!
//! Перенос `engine/rng.ts` бит в бит: тот же сид даёт ту же
//! последовательность, что и в эталоне. Арифметика JS (`Math.imul`, `|0`,
//! `>>>`) — это операции по модулю 2^32, здесь — `wrapping_*` над u32.
//! Дробные броски (`int`, `chance`) считаются в f64 в том же порядке
//! операций, что и в JS: округления совпадают.
//!
//! Потоки по метке: `stream("doors")` даёт независимый генератор, чьё зерно
//! выведено из зерна родителя и метки.

/// Перемешивание 32 бит (финализатор murmur3).
#[inline]
fn mix32(mut x: u32) -> u32 {
    x = (x ^ (x >> 16)).wrapping_mul(0x85eb_ca6b);
    x = (x ^ (x >> 13)).wrapping_mul(0xc2b2_ae35);
    x ^ (x >> 16)
}

/// Метка потока: строка (FNV-1a по кодовым единицам UTF-16, как `charCodeAt`) или число.
#[derive(Clone, Copy, Debug)]
pub enum Label<'a> {
    Str(&'a str),
    Num(u32),
}

impl<'a> From<&'a str> for Label<'a> {
    fn from(s: &'a str) -> Self {
        Label::Str(s)
    }
}

impl From<u32> for Label<'_> {
    fn from(n: u32) -> Self {
        Label::Num(n)
    }
}

fn hash_label(label: Label) -> u32 {
    match label {
        Label::Num(n) => mix32(n),
        Label::Str(s) => {
            let mut h: u32 = 0x811c_9dc5;
            for u in s.encode_utf16() {
                h = (h ^ u as u32).wrapping_mul(0x0100_0193);
            }
            h
        }
    }
}

/// Смешать два 32-битных числа (`mix` в solver/solve.ts).
#[inline]
pub fn mix_pair(a: u32, b: u32) -> u32 {
    let mut x = (a ^ b.wrapping_mul(0x9e37_79b1)).wrapping_mul(0x85eb_ca6b);
    x ^= x >> 13;
    x = x.wrapping_mul(0xc2b2_ae35);
    x ^ (x >> 16)
}

const TWO30: f64 = 1_073_741_824.0;
const INV30: f64 = 1.0 / TWO30;

#[derive(Clone, Debug)]
pub struct Rng {
    st: [u32; 2],
    /// Зерно раунда решателя (`DayRng` эталона): каждое утро ГСЧ
    /// перезаправляется от (раунд, день). `None` — обычный ГСЧ.
    day_round: Option<u32>,
}

impl Default for Rng {
    fn default() -> Self {
        Rng::new(1)
    }
}

impl Rng {
    pub fn new(seed: u32) -> Self {
        Rng { st: [seed, seed], day_round: None }
    }

    /// ГСЧ раунда решателя: утро дня day перезаправляет его от (round, day).
    pub fn day(seed: u32, round: u32) -> Self {
        Rng { st: [seed, seed], day_round: Some(round) }
    }

    /// Сменить зерно раунда (для `Rng::day`).
    #[inline]
    pub fn set_round(&mut self, round: u32) {
        self.day_round = Some(round);
    }

    /// Текущее состояние; можно сохранять и восстанавливать.
    #[inline]
    pub fn s(&self) -> u32 {
        self.st[0]
    }

    #[inline]
    pub fn set_s(&mut self, v: u32) {
        self.st[0] = v;
    }

    /// Начать заново с зерна.
    #[inline]
    pub fn seed(&mut self, seed: u32) -> &mut Self {
        self.st = [seed, seed];
        self
    }

    /// Следующее u32.
    #[inline]
    pub fn next(&mut self) -> u32 {
        self.st[0] = self.st[0].wrapping_add(0x6d2b_79f5);
        let mut t = self.st[0];
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        t ^ (t >> 14)
    }

    /// Следующие 30 бит (внутренние броски берут их, как в эталоне).
    #[inline]
    pub fn bits30(&mut self) -> u32 {
        self.next() >> 2
    }

    /// [0, 1), шаг 2^-30.
    #[inline]
    pub fn float(&mut self) -> f64 {
        self.bits30() as f64 * INV30
    }

    /// Целое [0, n), n ≤ 2^30.
    #[inline]
    pub fn int(&mut self, n: i32) -> i32 {
        (self.bits30() as f64 * INV30 * n as f64).floor() as i32
    }

    /// true с вероятностью p.
    #[inline]
    pub fn chance(&mut self, p: f64) -> bool {
        (self.bits30() as f64) < p * TWO30
    }

    /// Начало утра дня day. Обычный ГСЧ ничего не делает; ГСЧ раунда решателя
    /// перезаправляется от (раунд, день), чтобы с каждого утра кости у
    /// кандидатов снова совпадали.
    #[inline]
    pub fn morning(&mut self, day: i32) {
        if let Some(round) = self.day_round {
            self.seed(mix_pair(round, 0x10000u32.wrapping_add(day as u32)));
        }
    }

    /// Независимый поток по метке: тот же родитель и метка → те же числа.
    pub fn stream<'a>(&self, label: impl Into<Label<'a>>) -> Rng {
        Rng::new(mix32(self.st[1] ^ hash_label(label.into())))
    }

    /// Перезаправить существующий объект как поток родителя.
    pub fn stream_into<'a, 'r>(&self, label: impl Into<Label<'a>>, into: &'r mut Rng) -> &'r mut Rng {
        into.seed(mix32(self.st[1] ^ hash_label(label.into())))
    }
}
