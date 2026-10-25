//! Кадр игры → совет и отметки: прочитать доску, вести инструменты через забег, спросить советника, разложить
//! отметки. Перенос vorax-overlay/src/pipeline.rs (DinoRaos/vorax-solver-rs, MIT). Своё: данные чтения — снаружи
//! (`scan_data_ru` для русского клиента), отметки — в пикселях кадра вместе со сдвигом окна игры в нём.

use crate::data::ScanData;
use crate::layout::layout_at;
use crate::live::carried_round;
use crate::marks::{plan_for, Plan};
use crate::pixels::Img;
use crate::reading::{Problem, Reading};
use crate::scan::scan;
use crate::marks::{only_plan, title};
use crate::cards::read_phase;
use crate::titles::{pictures, ranked, read_titles, title_crop};
use crate::track::{track_instruments, InstrumentTracker};
use crate::window::{crop_to, windowed_game};
use gorn_vorax::advisor::{advise_plan, Advice, ScreenPhase};

pub type TextReader<'a> = Option<&'a mut dyn FnMut(&Img) -> String>;

/// Насколько опустить область заголовка (опорные пиксели), чтобы в неё попала подпись вида карты внизу.
const LABEL_DROP: f64 = 150.0;

/// Будущих на вариант у решателя для совета (как у эталона: 1024 — около 0,2 с на 12 потоках на самой трудной
/// доске; доходит до 840k в 81,5% партий).
pub const FUTURES: u32 = 1024;

pub enum Outcome {
    /// не экран «Хирургической подготовки»
    NotGame,
    /// карта ещё летит: прошлый совет в силе
    Animating,
    /// забег закрывается после последнего выбора 13-го раунда (анимация конца, награды): не доска
    Closing,
    /// совет по доске и его отметки; `origin` — где в кадре левый верхний угол прочитанного (окна игры)
    Advice { reading: Box<Reading>, advice: Box<Advice>, plan: Plan, origin: (f64, f64), read_ms: f64 },
    /// одна карта инструмента в начале забега: выбора нет, только отметить её; `name` — по её заголовку (OCR)
    Only { plan: Plan, origin: (f64, f64), name: Option<String>, text: String },
}

pub struct Pipeline {
    data: &'static ScanData,
    pub tracker: InstrumentTracker,
    pub shelley: i32,
    /// перебросов инструментов осталось, как их последний раз прочитали в этом забеге (на доске зелий их не видно)
    instrument_refreshes: Option<i32>,
    /// последняя доска, по которой советовали (её раунд — если раунд следующей не прочитан)
    last: Option<Reading>,
    /// забег дошёл до планов операции (раунды 11–13): доска после них без карт — его закрытие
    protocols: bool,
    /// экран Vorax открыли снова посреди того же забега (Escape и обратно), а не новый забег
    pub resumed: bool,
}

/// Продолжается ли идущий забег, по раунду доски до (`before`) и прочитанному наверняка сейчас (`now`). Новый
/// забег начинается с 1-го раунда, после 13-го ни один не продолжается.
fn resumes(before: Option<i32>, now: Option<i32>) -> bool {
    match (before, now) {
        (Some(b), Some(n)) => n > 1 && b < 13 && n >= b,
        _ => false,
    }
}

/// Перебросов инструментов в начале забега по уровню Интерна Шелли (src/data/shelley.ts эталона).
pub fn shelley_rerolls(level: i32) -> i32 {
    [0, 1, 1, 2, 2, 2].get((level - 1).clamp(0, 5) as usize).copied().unwrap_or(0)
}

impl Pipeline {
    pub fn new(data: &'static ScanData, shelley: i32) -> Pipeline {
        Pipeline { data, tracker: InstrumentTracker::default(), shelley, instrument_refreshes: None, last: None, protocols: false, resumed: false }
    }

    /// Перебросы инструментов: прочитанные на выборе инструмента, иначе с прошлого в этом забеге, иначе — сколько
    /// даёт уровень Шелли.
    fn fill_rerolls(&mut self, reading: &mut Reading) {
        match reading.state.phase {
            ScreenPhase::Instrument => {
                if reading.fields.get("refreshes").is_some_and(|f| f.sure) {
                    self.instrument_refreshes = reading.state.instrument_refreshes;
                }
            }
            _ => {
                if reading.state.instrument_refreshes.is_none() {
                    reading.state.instrument_refreshes = Some(self.instrument_refreshes.unwrap_or(shelley_rerolls(self.shelley)));
                }
            }
        }
    }

    /// `frame`: кадр с игрой; `trigger`: "screen", когда лог говорит, что открылся новый забег.
    pub fn run(&mut self, frame: &Img, trigger: Option<&str>, ocr: TextReader) -> Outcome {
        let t = std::time::Instant::now();
        self.resumed = false;
        let data = self.data;
        let area = windowed_game(frame);
        let img = match &area {
            Some(a) => crop_to(frame, a),
            None => frame.clone(),
        };
        let origin = area.map_or((0.0, 0.0), |a| (a.x.round().max(0.0), a.y.round().max(0.0)));
        // с OCR: карты по названиям, по картинкам — только где название не решило
        let first = scan(&img, data, ocr.is_none());
        if first.has(|p| matches!(p, Problem::OneCard)) {
            return self.only(&img, &first, origin, trigger, ocr);
        }
        let raw = match ocr {
            Some(read) => {
                let (dy, ui) = (first.hand_dy.unwrap_or(0.0), first.ui());
                let pics = |phase: &str, size: usize, slots: &[usize]| pictures(&img, data, phase, size, slots, dy, ui);
                read_titles(&img, first, data, read, Some(&pics))
            }
            None => first,
        };
        if raw.has(|p| matches!(p, Problem::Animating)) {
            return Outcome::Animating;
        }
        if raw.has(|p| matches!(p, Problem::NotFound)) {
            return Outcome::NotGame;
        }
        let round_sure = raw.fields.get("round").is_some_and(|f| f.sure);
        let before = self.last.as_ref().and_then(|r| r.state.round);
        self.resumed = trigger == Some("screen") && resumes(before, round_sure.then_some(raw.state.round.unwrap_or(1)));
        let trigger = if self.resumed { None } else { trigger };
        if trigger == Some("screen") {
            self.instrument_refreshes = None;
            self.protocols = false;
        }
        let protocol = raw.state.phase == ScreenPhase::Protocol;
        if self.protocols && !protocol && raw.state.hand.is_empty() {
            return Outcome::Closing;
        }
        self.protocols |= protocol;
        let tr = track_instruments(&self.tracker, &raw, trigger);
        self.tracker = tr.tracker.clone();
        let mut reading = raw;
        reading.state.instruments = tr.instruments.clone();
        reading.state.instrument_rounds = Some(tr.rounds.clone());
        // раунд, прочитанный не наверняка (или невозможный), — с доски до, а не 1-й
        let read_round = reading.state.round.unwrap_or(1);
        let impossible = (protocol && read_round < 11) || (self.resumed && before.is_some_and(|b| read_round < b));
        if let Some(f) = reading.fields.get("round").filter(|f| !f.sure || impossible) {
            let before = self.last.as_ref().map_or(Some(1), |r| r.state.round);
            reading.state.round = Some(carried_round(before, reading.state.phase, f.guess.as_deref()));
        }
        reading.state.shelley = Some(self.shelley);
        self.fill_rerolls(&mut reading);
        let read_ms = t.elapsed().as_secs_f64() * 1000.0;
        let advice = advise_plan(&reading.state, Some(FUTURES), true);
        let l = layout_at(img.width, img.height, reading.ui()).shifted_hand(reading.hand_dy.unwrap_or(0.0));
        let plan = plan_for(&advice, &reading.state, &reading, &l, data);
        self.last = Some(reading.clone());
        Outcome::Advice { reading: Box::new(reading), advice: Box::new(advice), plan, origin, read_ms }
    }

    /// Не в эталоне: одна карта посередине. Инструмент (по подписи внизу карты — «Хирургические инструменты»; без
    /// OCR — по заголовку выбора) — единственный вариант: отметить, назвать по заголовку карты. Иначе — карта в
    /// полёте, как у эталона. На открытии экрана это начало забега: учёт инструментов и раундов — заново.
    fn only(&mut self, img: &Img, r: &Reading, origin: (f64, f64), trigger: Option<&str>, ocr: TextReader) -> Outcome {
        // Новая доска пришла, а на экране ещё начальный инструмент (25.10: подсказка «Выводковая камера» висела над
        // розданными зельями) — это его уход, а не выбор: посмотреть снова.
        if trigger == Some("board") {
            return Outcome::Animating;
        }
        let l = layout_at(img.width, img.height, r.ui()).shifted_hand(r.hand_dy.unwrap_or(0.0));
        let c = l.cards[1];
        let (instrument, text) = match ocr {
            Some(read) => {
                // область заголовка, опущенная к низу карты: там подпись вида карты
                let mut low = c;
                low.cy += LABEL_DROP * l.k;
                let label = read(&title_crop(img, &low, l.k)).to_lowercase();
                let instrument = label.contains("инструмент") || label.contains("instrument");
                (instrument, if instrument { read(&title_crop(img, &c, l.k)) } else { String::new() })
            }
            None => (read_phase(img, &l, self.data).0.as_deref() == Some("instrument"), String::new()),
        };
        if !instrument {
            return Outcome::Animating;
        }
        let name = ranked(self.data, "instrument", &text).into_iter().next().filter(|(_, q)| *q >= 6.0).map(|(n, _)| n);
        if trigger == Some("screen") {
            self.tracker = InstrumentTracker::default();
            self.instrument_refreshes = None;
            self.protocols = false;
            self.last = None;
        }
        let shown = name.as_deref().map(|n| title(self.data, ScreenPhase::Instrument, n));
        Outcome::Only { plan: only_plan(&l, shown.as_deref()), origin, name, text }
    }

    /// Выбор, в котором учёт инструментов ещё не уверен: стоит посмотреть ещё раз, когда иконка встанет на место.
    pub fn wants_relook(&self) -> bool {
        self.tracker.unsure.iter().any(|q| q.looks == 0)
    }
}

#[cfg(test)]
mod tests {
    use super::resumes;

    #[test]
    fn the_screen_opened_again_mid_game_goes_on() {
        assert!(resumes(Some(6), Some(6)));
        assert!(resumes(Some(6), Some(7)));
        assert!(resumes(Some(12), Some(12)));
    }

    #[test]
    fn a_new_game_is_a_new_game() {
        assert!(!resumes(Some(13), Some(1)));
        assert!(!resumes(Some(6), Some(1)));
        assert!(!resumes(Some(13), Some(13)));
        assert!(!resumes(Some(8), Some(5)));
        assert!(!resumes(Some(6), None));
        assert!(!resumes(None, Some(6)));
    }
}
