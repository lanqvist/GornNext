//! Синтетический лог игры для сверки трекера: настоящие формы строк (из
//! логов и тестов эталона), перемешанные случайно — карты и укрытие, тюрьма,
//! сумка (InitBagData, Modfy, Add, Remove) в рамках протоколов, биржа,
//! пробивки цен (с фильтрами, несколькими валютами, оборванным дампом),
//! профиль игрока, мусорные строки, разные переводы строк.

use std::fmt::Write as _;

struct R(u64);

impl R {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
    fn chance(&mut self, p: f64) -> bool {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64 * 1.0 < p
    }
    fn pick<'a, T>(&mut self, v: &'a [T]) -> &'a T {
        &v[self.below(v.len() as u64) as usize]
    }
}

const CATS: [&str; 4] = ["TLLua", "TLShipping", "TLNetGame", "GameLog"];
const MAPS: [(&str, &str, i64, i64); 6] = [
    ("Content/Art/Maps/01SD/SD_YuanGuTongDao201", "SD_YuanGuTongDao201", 5348, 3),
    ("Content/Art/Maps/Boss/ChiGuiWuShi", "ChiGuiWuShi", 5351, 3),
    ("Content/Art/Maps/S2/XuHaiZhongGang", "XuHaiZhongGang", 5055, 3),
    ("Content/Art/Maps/03JH/JH_TongKuMiDian", "JH_TongKuMiDian", 506, 3),
    ("Content/Art/Season/S16/Maps/ZhongZuiDiYu", "ZhongZuiDiYu", 235501, 28),
    ("Content/Art/Maps/01SD/XZ_YuJinZhiXiBiNanSuo200", "XZ_YuJinZhiXiBiNanSuo200", 110, 0),
];
const PROTOS: [&str; 14] = [
    "PickItem", "PickItems", "Spv3Open", "Push2", "XchgForSale", "XchgReceive", "XchgBuy", "XchgRecall", "PutOn",
    "Evaluate", "ReCostItem", "BuyVendorGoods", "MysteryProto", "OpenRewardBox",
];

pub struct Gen {
    r: R,
    t: i64,
    frame: u32,
    out: String,
    ends: u8,
    items: Vec<i64>,
    keys: Vec<i64>,
}

impl Gen {
    fn stamp(&mut self) -> String {
        self.t += self.r.below(1500) as i64;
        self.frame = (self.frame + 1) % 1000;
        let s = self.t / 1000;
        let (d, h, m, sec) = (20 + s / 86_400 % 5, s / 3600 % 24, s / 60 % 60, s % 60);
        format!("[2026.10.{d:02}-{h:02}.{m:02}.{sec:02}:{:03}][{:>3}]", self.t % 1000, self.frame)
    }

    fn eol(&mut self) {
        let e = match self.ends {
            0 => "\n",
            1 => "\r\n",
            _ => *self.r.pick(&["\n", "\r\n", "\r\n", "\r"]),
        };
        self.out.push_str(e);
    }

    fn line(&mut self, body: &str) {
        let st = self.stamp();
        let cat = *self.r.pick(&CATS);
        let _ = write!(self.out, "{st}{cat}: Display: [Game] {body}");
        self.eol();
    }

    fn raw(&mut self, body: &str) {
        self.out.push_str(body);
        self.eol();
    }

    fn item(&mut self) -> i64 {
        if self.r.chance(0.3) {
            100300
        } else if self.r.chance(0.2) {
            *self.r.pick(&self.keys.clone())
        } else {
            *self.r.pick(&self.items.clone())
        }
    }

    fn enter(&mut self) {
        let (path, asset, level, ty) = *self.r.pick(&MAPS);
        let s = format!("LevelMgr@ LevelUid, LevelType, LevelId = {} {ty} {level}", self.r.below(2_000_000));
        self.line(&s);
        self.line(&format!("LevelMgr@:LevelPath, Model = {path}/{asset}.umap World'/Game/{asset}'"));
        if self.r.chance(0.9) {
            self.line(&format!("LevelMgr:_EnterLevelSuccess() {level}"));
        }
    }

    fn bag(&mut self, slots: &mut Vec<(i64, i64, i64, i64)>) {
        let proto = *self.r.pick(&PROTOS);
        self.line(&format!("ItemChange@ ProtoName={proto} start"));
        for _ in 0..1 + self.r.below(4) {
            let k = self.r.below(8);
            if slots.is_empty() || k == 0 {
                let (p, s) = (102 + self.r.below(2) as i64, self.r.below(60) as i64);
                let id = self.item();
                let n = 1 + self.r.below(50) as i64;
                if self.r.chance(0.5) {
                    let s = format!("ItemChange@ Add Id={id}_25b2b109-aa{} BagNum={n} in PageId={p} SlotId={s}", self.r.below(99));
                    self.line(&s);
                }
                self.line(&format!("BagMgr@:Modfy BagItem PageId = {p} SlotId = {s} ConfigBaseId = {id} Num = {n}"));
                slots.push((p, s, id, n));
            } else {
                let i = self.r.below(slots.len() as u64) as usize;
                let (p, s, id, n) = slots[i];
                if k == 1 {
                    self.line(&format!("BagMgr@:RemoveBagItem PageId = {p} SlotId = {s}"));
                    slots.remove(i);
                } else {
                    let id = if self.r.chance(0.05) { self.item() } else { id };
                    let n2 = (n + self.r.below(40) as i64 - 15).max(0);
                    self.line(&format!("BagMgr@:Modfy BagItem PageId = {p} SlotId = {s} ConfigBaseId = {id} Num = {n2}"));
                    slots[i] = (p, s, id, n2);
                }
            }
        }
        if self.r.chance(0.95) {
            self.line(&format!("ItemChange@ ProtoName={proto} end"));
        }
    }

    fn price(&mut self) {
        let syn = self.r.below(200);
        let refer = self.item();
        self.line(&format!("----Socket SendMessage STT----XchgSearchPrice----SynId = {syn}"));
        self.line("");
        self.raw("+typ3 [13]");
        self.raw(&format!("+filters+1+refer [{refer}]"));
        self.raw("|       | +key [16]");
        match self.r.below(4) {
            0 => {
                self.raw("|       +6+params+1+min [36]");
                self.raw("|       | +refer [101010089_d1_mt31]");
            }
            1 => {
                self.raw("|       +3+params+1+max [0]");
                self.raw("|       | |      | +min [0]");
                self.raw("|       | +refer [0]");
            }
            _ => {}
        }
        self.line("----Socket SendMessage End----");
        let syn2 = if self.r.chance(0.9) { syn } else { self.r.below(200) };
        self.line(&format!("----Socket RecvMessage STT----XchgSearchPrice----SynId = {syn2}"));
        if self.r.chance(0.5) {
            self.line("");
        }
        let groups = 1 + self.r.below(2);
        for g in 1..=groups {
            let cur = if self.r.chance(0.7) { 100300 } else { 100200 };
            if self.r.chance(0.8) {
                let pre = if g == 1 { "+prices+" } else { "|      +" };
                self.raw(&format!("{pre}{g}+currency [{cur}]"));
                let s = format!("|      | +unitPrices+1 [{}.{}]", self.r.below(500), self.r.below(10));
                self.raw(&s);
            } else {
                let pre = if g == 1 { "+prices+" } else { "|      +" };
                let s = format!("{pre}{g}+unitPrices+1 [0.000{}]", 1 + self.r.below(9));
                self.raw(&s);
            }
            for k in 2..2 + self.r.below(5) {
                let s = format!("|      | |          +{k} [{}]", self.r.below(1000) as f64 / 7.0);
                self.raw(&s);
            }
        }
        if self.r.chance(0.92) {
            self.line("----Socket RecvMessage End----");
        }
    }

    fn player(&mut self) {
        let id = format!("95608272857761{:04}", self.r.below(3));
        if self.r.chance(0.5) {
            self.line(&format!(
                "PlayerIdTrace@SelectRoleNetMgr:_UserLoginStep3 buildLoginParm playerId={id} saveInfoPlayerId={id} userId=1"
            ));
        }
        let s = format!("----Socket RecvMessage STT----GetPlayerData----SynId = {}", self.r.below(50));
        self.line(&s);
        self.line("");
        self.raw("+skills+1+key [104]");
        self.raw("|      | +value+level [24]");
        if self.r.chance(0.9) {
            self.raw("+player+SkillCurCost [455]");
            let s = format!("|      +SeasonId [{}]", if self.r.chance(0.8) { 1601 } else { 1501 });
            self.raw(&s);
            self.raw(&format!("|      +PlayerId [{id}]"));
            let s = format!("|      +HeroId [{}]", 1700 + self.r.below(3) * 100);
            self.raw(&s);
            let s = format!("|      +Level [{}]", 60 + self.r.below(40));
            self.raw(&s);
            if self.r.chance(0.8) {
                self.raw("|      +Name [VLPZDC#3648]");
            }
        }
        self.line("----Socket RecvMessage End----");
    }

    fn noise(&mut self) {
        let n = self.r.below(6);
        match n {
            0 => self.line("PageStack@ CurRunView = 4150_S16BoardMainCtrl"),
            1 => self.raw(""),
            2 => self.line("Мусор: «строка» с юникодом — ничего не значит"),
            3 => self.line("LevelMgr@ что-то без чисел"),
            4 => self.line("BagMgr@:Modfy BagItem PageId = x"),
            _ => self.line("Normal line"),
        }
    }
}

/// Лог из lines строк (примерно) по зерну seed.
pub fn generate(seed: u64, lines: usize, items: &[i64], keys: &[i64]) -> Vec<u8> {
    let mut g = Gen {
        r: R(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1),
        t: 1_760_000_000_000 % 86_400_000,
        frame: 0,
        out: String::new(),
        ends: (seed % 3) as u8,
        items: items.to_vec(),
        keys: keys.to_vec(),
    };
    if seed % 2 == 0 {
        g.out.push('\u{feff}');
    }
    g.raw("Log file open, 10/20/26 13:52:35");
    let mut slots = vec![];
    // Полная синхронизация страницы.
    for s in 0..10 {
        let id = g.item();
        let n = 1 + g.r.below(100) as i64;
        g.line(&format!("BagMgr@:InitBagData PageId = 102 SlotId = {s} ConfigBaseId = {id} Num = {n}"));
        slots.push((102, s, id, n));
    }
    while g.out.matches('\n').count() + g.out.matches('\r').count() < lines * 2 {
        match g.r.below(20) {
            0..=1 => g.enter(),
            2..=11 => g.bag(&mut slots),
            12..=13 => g.price(),
            14 => g.player(),
            _ => g.noise(),
        }
        if g.out.len() > lines * 90 {
            break;
        }
    }
    // Хвост без перевода строки — проверка режима текущего лога.
    if seed % 3 == 0 {
        let st = g.stamp();
        let _ = write!(g.out, "{st}TLLua: Display: [Game] BagMgr@:Modfy BagItem PageId = 102 SlotId = 0 ConfigBaseId = 100300 Num = 1");
    }
    g.out.into_bytes()
}
