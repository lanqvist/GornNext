//! Тюрьма Талоса: ид, типы и параметры правил.
//!
//! Перенос `src/core/prison/model.ts` (gorn-tli) — только то, что нужно
//! движку и мгновенной стратегии. Порядок ид тот же: номера — индексы в
//! массивах и битах маски, сверка с эталоном опирается на них.

// ── звери и редкости ────────────────────────────────────────────────────────

pub const BEASTS: [&str; 4] = ["mammoth", "lion", "wolf", "rat"];

pub const MAMMOTH: i32 = 0;
pub const LION: i32 = 1;
pub const WOLF: i32 = 2;
pub const RAT: i32 = 3;

pub const LEGENDARY: i32 = 3;
pub const SUPREME: i32 = 4;
pub const PRISMATIC: i32 = 5;

/// Камер на кольце; 0 — допросная.
pub const CELLS: usize = 16;
pub const OFFICE: i32 = 0;

// ── орудия пыток (rules.md §7), порядок = номер бита в маске ────────────────

pub const TOOL_IDS: [&str; 31] = [
    // обход и двери
    "forged", "keys", "dogtag", "blacklist", "fire", "hook", "shortchain", "shackles", "stake",
    // пожирание
    "whip", "saw", "collar", "muzzle", "revenge", "cage", "skewer", "crown", "maul", "guillotine", "ravenous",
    // замена
    "rope", "pestle", "scales", "axe", "maiden", "scapegoat",
    // утро
    "tongs", "claws", "ironbox", "coin", "trap",
];
pub const TOOL_COUNT: usize = TOOL_IDS.len();

/// Номер бита орудия: `tool::FORGED == 0` … `tool::TRAP == 30`.
pub mod tool {
    pub const FORGED: u32 = 0;
    pub const KEYS: u32 = 1;
    pub const DOGTAG: u32 = 2;
    pub const BLACKLIST: u32 = 3;
    pub const FIRE: u32 = 4;
    pub const HOOK: u32 = 5;
    pub const SHORTCHAIN: u32 = 6;
    pub const SHACKLES: u32 = 7;
    pub const STAKE: u32 = 8;
    pub const WHIP: u32 = 9;
    pub const SAW: u32 = 10;
    pub const COLLAR: u32 = 11;
    pub const MUZZLE: u32 = 12;
    pub const REVENGE: u32 = 13;
    pub const CAGE: u32 = 14;
    pub const SKEWER: u32 = 15;
    pub const CROWN: u32 = 16;
    pub const MAUL: u32 = 17;
    pub const GUILLOTINE: u32 = 18;
    pub const RAVENOUS: u32 = 19;
    pub const ROPE: u32 = 20;
    pub const PESTLE: u32 = 21;
    pub const SCALES: u32 = 22;
    pub const AXE: u32 = 23;
    pub const MAIDEN: u32 = 24;
    pub const SCAPEGOAT: u32 = 25;
    pub const TONGS: u32 = 26;
    pub const CLAWS: u32 = 27;
    pub const IRONBOX: u32 = 28;
    pub const COIN: u32 = 29;
    pub const TRAP: u32 = 30;
}

/// Номер орудия по ид, `None` — нет такого.
pub fn tool_index(id: &str) -> Option<u32> {
    TOOL_IDS.iter().position(|&t| t == id).map(|i| i as u32)
}

/// Строчные буквы и цифры через один пробел, «ё» как «е»: название с карты
/// (OCR) и из TOOL_NAMES сравниваются в одном виде.
fn plain_title(s: &str) -> Vec<char> {
    let mut out: Vec<char> = vec![];
    for ch in s.chars().flat_map(char::to_lowercase) {
        let ch = if ch == 'ё' { 'е' } else { ch };
        if ch.is_alphanumeric() {
            out.push(ch);
        } else if out.last().is_some_and(|&c| c != ' ') {
            out.push(' ');
        }
    }
    while out.last() == Some(&' ') {
        out.pop();
    }
    out
}

fn edit_distance(a: &[char], b: &[char]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, &x) in a.iter().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, &y) in b.iter().enumerate() {
            cur[j + 1] = (prev[j] + usize::from(x != y)).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[b.len()]
}

/// Орудие по названию с карты выбора (текст OCR). Длинные названия клиент
/// обрезает многоточием («Пыточный намордник бешеного...»), поэтому название
/// сравнивается по началу той же длины; ошибки распознавания — до четверти
/// букв. Лучшее должно заметно опережать второе, иначе None.
pub fn tool_by_title(text: &str) -> Option<u32> {
    let t = plain_title(text);
    if t.len() < 6 {
        return None;
    }
    let mut scored: Vec<(f64, u32)> = TOOL_NAMES
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let n = plain_title(name);
            let k = t.len().min(n.len());
            let d = edit_distance(&t[..k], &n[..k]) + t.len().saturating_sub(n.len());
            (1.0 - d as f64 / t.len() as f64, i as u32)
        })
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let (best, id) = scored[0];
    (best >= 0.75 && best - scored[1].0 >= 0.15).then_some(id)
}

/// Заголовок окна события — «Цепная реакция» (OCR, с ошибками распознавания):
/// название где-то в строке, до трёх правок.
pub fn is_chain_reaction(text: &str) -> bool {
    let t = plain_title(text);
    ["цепная реакция", "chain reaction"].iter().any(|name| {
        let n: Vec<char> = name.chars().collect();
        t.len() >= n.len() && t.windows(n.len()).any(|w| edit_distance(w, &n) <= 3)
    })
}

// ── подарки сокровищницы (§9) ───────────────────────────────────────────────

pub const GIFT_IDS: [&str; 4] = ["ring", "horn", "wine", "brand"];

/// Русские названия орудий (по номеру), как TOOL_NAMES эталона.
pub const TOOL_NAMES: [&str; 31] = [
    "Поддельный ключ",
    "Потерянная связка ключей",
    "Жетон каторжника",
    "Чёрный список экзекутора",
    "Раскалённое клеймо",
    "Крюк отбора",
    "Кандалы с короткой цепью",
    "Кандалы с длинной цепью",
    "Столб для сожжения",
    "Шипастая железная плеть",
    "Ржавая пила",
    "Шипастый ошейник безумного зверя",
    "Пыточный намордник бешеного зверя",
    "Железная дева мстителя",
    "Переполненная клетка зверей",
    "Шампур крысиных голов: Огромная крыса",
    "Железная корона короля тюрьмы: Яростный лев",
    "Молот Слоновья нога: Слон",
    "Гильотина с волчьими клыками: Вожак",
    "Голодная удушающая цепь лютого зверя",
    "Висельная петля",
    "Железный пест",
    "Весы тюремщика",
    "Палаческая секира надзирателя",
    "Рассечённый саркофаг",
    "Гильотина козла отпущения",
    "Клещи порока: Слон",
    "Грязный железный коготь: Вожак",
    "Кишащий железный ящик: Огромная крыса",
    "Монета участи братьев",
    "Окровавленный капкан: Яростный лев",
];

/// Названия подарков в клиенте (по номеру), как GIFT_NAMES эталона.
pub const GIFT_NAMES: [&str; 4] = [
    "Трёхглавое кольцо",
    "Медный боевой рог",
    "Багряное вино",
    "Клеймо первородного греха",
];

// ── события (§8) ────────────────────────────────────────────────────────────

pub const EVENT_IDS: [&str; 17] = [
    "chain", "execution", "behind", "appetite", "convict", "raven", "freedom", "usurp1", "usurp2", "usurp3",
    "usurp4", "ratway", "treasure", "rival", "luck", "escort", "extra",
];

pub mod event {
    pub const CHAIN: i32 = 0;
    pub const EXECUTION: i32 = 1;
    pub const BEHIND: i32 = 2;
    pub const APPETITE: i32 = 3;
    pub const CONVICT: i32 = 4;
    pub const RAVEN: i32 = 5;
    pub const FREEDOM: i32 = 6;
    pub const USURP1: i32 = 7;
    pub const USURP2: i32 = 8;
    pub const USURP3: i32 = 9;
    pub const USURP4: i32 = 10;
    pub const RATWAY: i32 = 11;
    pub const TREASURE: i32 = 12;
    pub const RIVAL: i32 = 13;
    pub const LUCK: i32 = 14;
    pub const ESCORT: i32 = 15;
    pub const EXTRA: i32 = 16;
}

/// Постоянная часть пула событий (номера). Узурпация — одна текущая ступень, её держит состояние.
pub const EVENT_POOL: [i32; 13] = [
    event::CHAIN,
    event::EXECUTION,
    event::BEHIND,
    event::APPETITE,
    event::CONVICT,
    event::RAVEN,
    event::FREEDOM,
    event::RATWAY,
    event::TREASURE,
    event::RIVAL,
    event::LUCK,
    event::ESCORT,
    event::EXTRA,
];

// ── цели и стартовые события (§10) ──────────────────────────────────────────

pub const OBJECTIVE_IDS: [&str; 5] = ["apex", "jungle", "doors", "walk", "currents"];

pub mod objective {
    pub const APEX: i32 = 0;
    pub const JUNGLE: i32 = 1;
    pub const DOORS: i32 = 2;
    pub const WALK: i32 = 3;
    pub const CURRENTS: i32 = 4;
}

/// Особые темы забега (клиент 301–304).
pub const START_EVENT_IDS: [&str; 5] = ["none", "cagefight", "ironlaw", "tyranny", "abundance"];

pub mod start_event {
    pub const NONE: i32 = 0;
    pub const CAGEFIGHT: i32 = 1;
    pub const IRONLAW: i32 = 2;
    pub const TYRANNY: i32 = 3;
    pub const ABUNDANCE: i32 = 4;
}

// ── параметры правил ────────────────────────────────────────────────────────

/// Доли по редкостям 0…5.
pub type TierOdds = [f64; 6];

/// Числа правил; источники — в комментариях `Params` в model.ts эталона.
#[derive(Clone, Debug)]
pub struct Params {
    pub days: i32,
    pub iron_law_days: i32,
    pub storage: i32,
    pub rerolls: i32,
    pub door_odds: TierOdds,
    pub replace_odds: TierOdds,
    pub door_cards: i32,
    pub one_less_door: f64,
    pub door_reach: i32,
    pub start_open: i32,
    pub event_min: i32,
    pub door_event: f64,
    pub relock_event: f64,
    pub jungle_counts_bites: bool,
    pub objective_repeat: bool,
    /// По номеру цели: apex, jungle, doors, walk, currents.
    pub objective_target: [i32; 5],
    pub tyranny_extra: i32,
    pub abundance_gifts: i32,

    pub forged_chance: f64,
    pub keys_chance: f64,
    pub dogtag_chance: f64,
    pub blacklist_locked: i32,
    pub blacklist_count: i32,
    pub fire_chance: f64,
    pub shortchain_steps: i32,
    pub shortchain_chance: f64,
    pub shackles_steps: i32,
    pub shackles_chance: f64,
    pub stake_chance: f64,

    pub whip_chance: f64,
    pub saw_chance: f64,
    pub collar_chance: f64,
    pub muzzle_left: i32,
    pub muzzle_bonus: i32,
    pub revenge_chance: f64,
    pub cage_chance: f64,
    pub oppress_step: f64,
    pub skewer_bonus: i32,
    pub crown_bonus: i32,
    pub ravenous_chance: f64,

    pub scales_chance: f64,
    pub axe_opens: i32,
    pub maiden_bonus: i32,
    pub scapegoat_removes: i32,

    pub tongs_targets: i32,
    pub claws_gain: i32,
    pub ironbox_days: i32,
    pub ironbox_tier: i32,
    pub coin_chance: f64,
    pub coin_bonus: i32,
    pub trap_mornings: i32,

    pub appetite_tries: i32,
    pub appetite_chance: f64,
    pub raven_cells: i32,
    pub freedom_opens: i32,
    pub usurp_next_weight: i32,
    pub usurp_count: [i32; 4],
    pub usurp_gain: [i32; 4],
    pub extra_count: i32,

    pub ring_opens: i32,
    pub brand_count: i32,

    pub instigate_doors: i32,

    /// После замены ряд решений открывается снова (Пожирание в тот же день).
    /// false — прежняя модель: замена завершает день (только для A/B самоигрой).
    pub swap_then_devour: bool,
    /// «Чёрный список» считает запертых до открытия двери; false — прежняя
    /// модель: после открытия (только для A/B самоигрой).
    pub blacklist_before_open: bool,
    /// Цель «Пройти шаги», «Открыть камеры», «Скрытые волнения» засчитывается по
    /// клику на дверь, до хода надзирателя: награда приходит раньше обхода и
    /// действует на него; «Волнения» — дверь с «!», а не «Принять». false —
    /// прежняя модель: после хода, «Волнения» — по принятому событию.
    pub reward_on_click: bool,
    /// Итог забега: призмы (до `storage`) + столько за каждого Величайшего
    /// (красного) на доске в конце — финальный красный сундук [игрок, 22.10:
    /// после 10 призм надо копить красных]. Вес — предварительный.
    pub red_value: f64,

    // Варианты «[догадок]» правил для `gorn-cli rules-check` (этап 8, шаг 3);
    // по умолчанию — нынешнее толкование.
    /// «Рог»: едоки в случайном порядке (иначе — по номерам камер).
    pub horn_random_order: bool,
    /// «Рог»: едоки — те, у кого цель есть до первого укуса (иначе — проверка на
    /// ходу, правило до 22.10: по ходу Рога новые цели не появляются в счёт).
    pub horn_eaters_snapshot: bool,
    /// Укусы «Рога» запускают орудия Пожирания (Плеть, Пила, Дева).
    pub horn_devour_tools: bool,
    /// «Наградить»: на сколько повышается случайный узник.
    pub bestow_gain: i32,
    /// Плеть, Пила, Дева — только после первого укуса (не в цепочке).
    pub devour_tools_first_bite: bool,
    /// «Ошейник» и на первом укусе, не только в цепочке.
    pub collar_first_bite: bool,
    /// «Клетка»: новый узник той же редкости, что съеденный (иначе — по шансам двери).
    pub cage_same_tier: bool,
    /// «Железная дева»: узник по шансам двери (иначе — редкости едока).
    pub revenge_door_tier: bool,
    /// «Трёхглавое кольцо» открывает с «Крюком отбора» (не ниже Необычной);
    /// иначе — по шансам двери без Крюка (правило с 24.10).
    pub ring_hook: bool,
    /// «Рассечённый саркофаг»: Высших и Низших считают относительно нового
    /// узника (до 27.10); иначе — относительно заменяемого, до замены.
    pub maiden_new_tier: bool,
}

pub const DEFAULT_PARAMS: Params = Params {
    days: 20,
    iron_law_days: 30,
    storage: 10,
    rerolls: 1,
    door_odds: [0.418, 0.359, 0.114, 0.092, 0.018, 0.0],
    replace_odds: [0.422, 0.333, 0.145, 0.073, 0.023, 0.003],
    door_cards: 3,
    one_less_door: 0.0,
    door_reach: 6,
    start_open: 4,
    event_min: 1,
    door_event: 0.0,
    relock_event: 0.0,
    jungle_counts_bites: true,
    objective_repeat: false,
    objective_target: [1, 4, 10, 18, 2],
    tyranny_extra: 3,
    abundance_gifts: 3,

    forged_chance: 0.5,
    keys_chance: 0.8,
    dogtag_chance: 0.25,
    blacklist_locked: 5,
    blacklist_count: 2,
    fire_chance: 0.35,
    shortchain_steps: 3,
    shortchain_chance: 0.5,
    shackles_steps: 4,
    shackles_chance: 0.4,
    stake_chance: 0.4,

    whip_chance: 0.35,
    saw_chance: 0.25,
    collar_chance: 0.8,
    muzzle_left: 4,
    muzzle_bonus: 2,
    revenge_chance: 0.3,
    cage_chance: 0.4,
    oppress_step: 0.05,
    skewer_bonus: 2,
    crown_bonus: 2,
    ravenous_chance: 0.4,

    scales_chance: 0.8,
    axe_opens: 2,
    maiden_bonus: 3,
    scapegoat_removes: 2,

    tongs_targets: 4,
    claws_gain: 3,
    ironbox_days: 2,
    ironbox_tier: 2,
    coin_chance: 0.6,
    coin_bonus: 2,
    trap_mornings: 3,

    appetite_tries: 5,
    appetite_chance: 0.5,
    raven_cells: 4,
    freedom_opens: 3,
    usurp_next_weight: 10,
    usurp_count: [1, 1, 2, -1],
    usurp_gain: [1, 2, 2, 2],
    extra_count: 3,

    ring_opens: 3,
    brand_count: 4,

    instigate_doors: 1,

    swap_then_devour: true,
    blacklist_before_open: true,
    // Обход → выбор орудия: надзиратель на старом месте [журнал фактов 20–22.10:
    // 20 из 20], у «Волнений» дверь выбранного обхода — с «!» (9 из 9).
    reward_on_click: true,
    red_value: 0.25,

    horn_random_order: false,
    // «все узники с целью Пожирания» — до первого укуса [журнал фактов 20–21.10:
    // проверка на ходу не объясняет 2 перехода, rules-check +15,3].
    horn_eaters_snapshot: true,
    horn_devour_tools: true,
    bestow_gain: 1,
    devour_tools_first_bite: false,
    collar_first_bite: false,
    cage_same_tier: false,
    revenge_door_tier: false,
    ring_hook: false,
    // «Саркофаг» — относительно заменяемого узника [журнал фактов 25–26.10:
    // 19 замен с Саркофагом — прежний узник объясняет все, новый ошибается в 3].
    maiden_new_tier: false,
};

#[cfg(test)]
mod tests {
    use super::*;

    /// Названия с карт выбора 26.10 (21:9), как их обрезает клиент, и ошибки OCR.
    #[test]
    fn tool_by_title_reads_card_names() {
        let id = |t: &str| tool_by_title(t).map(|i| TOOL_IDS[i as usize]);
        assert_eq!(id("Пыточный намордник бешеного..."), Some("muzzle"));
        assert_eq!(id("Пыточный намордник\nбешеного..."), Some("muzzle"));
        assert_eq!(id("Кишащий железный ящик: Огро..."), Some("ironbox"));
        assert_eq!(id("Железная корона короля тюрьм..."), Some("crown"));
        assert_eq!(id("Рассеченный саркофаг"), Some("maiden"));
        assert_eq!(id("Пыточныи наморднпк бешеного"), Some("muzzle"));
        assert_eq!(id("Висельная петля"), Some("rope"));
        // Две «Гильотины» — по началу не различить.
        assert_eq!(id("Гильотина"), None);
        assert_eq!(id("Условие срабатывания"), None);
        assert_eq!(id(""), None);
        assert!(is_chain_reaction("Цепная реакция"));
        // Латинские «e» вместо кириллических — частая ошибка OCR.
        assert!(is_chain_reaction("Цeпная рeакция"));
        assert!(is_chain_reaction("» Цепная реакцпя"));
        assert!(!is_chain_reaction("Казнь"));
        assert!(!is_chain_reaction(""));
    }
}
