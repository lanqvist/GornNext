//! Эталоны распознавания: `data/templates/*.json` (копия
//! `recognize/templates` эталона) → массивы f32. Перенос `recognize/templates.ts`.
//! Вектор в JSON — «масштаб;base64»: int8, делённые на 127 и умноженные на масштаб.

use std::sync::OnceLock;

use serde_json::Value;

use super::text::DigitSet;

pub fn decode_b64(s: &str) -> Vec<u8> {
    fn val(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a' + 26) as u32),
            b'0'..=b'9' => Some((c - b'0' + 52) as u32),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for c in s.bytes() {
        let Some(v) = val(c) else { continue };
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    out
}

pub fn unpack(s: &str) -> Vec<f32> {
    let (scale, data) = s.split_once(';').expect("эталон: «масштаб;base64»");
    let k = scale.parse::<f64>().expect("эталон: масштаб") / 127.0;
    decode_b64(data).iter().map(|&b| (b as i8 as f64 * k) as f32).collect()
}

#[derive(Clone, Debug)]
pub struct Proto {
    pub label: String,
    pub v: Vec<f32>,
}

/// Лучший прототип для вектора: класс и сходство.
pub fn nearest<'a>(v: &[f32], protos: &'a [Proto]) -> (&'a str, f64) {
    let mut label = "";
    let mut score = -2.0;
    for p in protos {
        let s = super::sample::dot(v, &p.v);
        if s > score {
            score = s;
            label = &p.label;
        }
    }
    (label, score)
}

/// Лучшее сходство с прототипами каждого класса, в порядке первого появления класса.
pub fn scores_by_label<'a>(v: &[f32], protos: &'a [Proto]) -> Vec<(&'a str, f64)> {
    let mut out: Vec<(&str, f64)> = vec![];
    for p in protos {
        let s = super::sample::dot(v, &p.v);
        match out.iter_mut().find(|(l, _)| *l == p.label) {
            Some(e) => {
                if s > e.1 {
                    e.1 = s;
                }
            }
            None => {
                if s > -2.0 {
                    out.push((&p.label, s));
                }
            }
        }
    }
    out
}

pub fn get<'a>(m: &[(&'a str, f64)], label: &str) -> Option<f64> {
    m.iter().find(|(l, _)| *l == label).map(|e| e.1)
}

pub struct Grad {
    pub x0: f64,
    pub y0: f64,
    pub cell: f64,
    pub w: usize,
    pub h: usize,
    pub data: Vec<u8>,
}

pub struct BoardTemplate {
    pub grad: Grad,
    pub numbers: Vec<Vec<f32>>,
}

pub struct CellsTemplate {
    pub lock: Vec<f32>,
    pub jaw: Vec<f32>,
    /// Прототипы портретов: класс (зверь, unknown — «?», locked — пустая запертая камера).
    pub beasts: Vec<Proto>,
}

pub struct ScreensTemplate {
    pub headers: Vec<Proto>,
    pub cards: Vec<Proto>,
    pub tools: Vec<Proto>,
    pub chromatic: Vec<Proto>,
    pub objectives: Vec<Proto>,
    pub pick_art: Vec<Proto>,
    pub pick_titles: Vec<Proto>,
    pub pick_headers: Vec<Proto>,
    pub reroll: Vec<Vec<f32>>,
    pub chromatic_header: Vec<Vec<f32>>,
    pub gifts: Vec<Proto>,
    /// Метка «Добыча» на портрете (None — эталона нет).
    pub prey: Option<Vec<f32>>,
    /// Метка «Бивень» (Щипцы) на портрете — то же место, что у «Добычи».
    pub tusk: Option<Vec<f32>>,
}

/// Все эталоны (читаются один раз на процесс).
pub struct Templates {
    pub board: BoardTemplate,
    pub cells: CellsTemplate,
    pub screens: ScreensTemplate,
    pub digits: DigitSet,
}

fn protos(v: &Value) -> Vec<Proto> {
    v.as_array()
        .map(|a| {
            a.iter()
                .map(|p| Proto { label: p["label"].as_str().unwrap().to_string(), v: unpack(p["v"].as_str().unwrap()) })
                .collect()
        })
        .unwrap_or_default()
}

fn vecs(v: &Value) -> Vec<Vec<f32>> {
    v.as_array().map(|a| a.iter().map(|x| unpack(x.as_str().unwrap())).collect()).unwrap_or_default()
}

pub fn templates() -> &'static Templates {
    static T: OnceLock<Templates> = OnceLock::new();
    T.get_or_init(|| {
        let b: Value = serde_json::from_str(include_str!("../../data/templates/board.json")).unwrap();
        let g = &b["grad"];
        let board = BoardTemplate {
            grad: Grad {
                x0: g["x0"].as_f64().unwrap(),
                y0: g["y0"].as_f64().unwrap(),
                cell: g["cell"].as_f64().unwrap(),
                w: g["w"].as_u64().unwrap() as usize,
                h: g["h"].as_u64().unwrap() as usize,
                data: decode_b64(g["data"].as_str().unwrap()),
            },
            numbers: vecs(&b["numbers"]),
        };
        let c: Value = serde_json::from_str(include_str!("../../data/templates/cells.json")).unwrap();
        let cells = CellsTemplate {
            lock: unpack(c["lock"].as_str().unwrap()),
            jaw: unpack(c["jaw"].as_str().unwrap()),
            beasts: protos(&c["beasts"]),
        };
        let d: Value = serde_json::from_str(include_str!("../../data/templates/digits.json")).unwrap();
        let digits = DigitSet {
            protos: d["protos"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| (p["d"].as_u64().unwrap() as usize, unpack(p["v"].as_str().unwrap())))
                .collect(),
            aspect: d["aspect"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect(),
        };
        let mut s: Value = serde_json::from_str(include_str!("../../data/templates/screens.json")).unwrap();
        // Надписи английского клиента — те же разделы и метки, дописаны к русским
        // (иконки от языка не зависят): эталоны с кадров сбора, `gorn-cli texts`.
        let en: Value = serde_json::from_str(include_str!("../../data/templates/screens-en.json")).unwrap();
        for (k, v) in en.as_object().unwrap() {
            if let (Some(list), Some(add)) = (s[k.as_str()].as_array_mut(), v.as_array()) {
                list.extend(add.iter().cloned());
            }
        }
        let screens = ScreensTemplate {
            headers: protos(&s["headers"]),
            cards: protos(&s["cards"]),
            tools: protos(&s["tools"]),
            chromatic: protos(&s["chromatic"]),
            objectives: protos(&s["objectives"]),
            pick_art: protos(&s["pickArt"]),
            pick_titles: protos(&s["pickTitles"]),
            pick_headers: protos(&s["pickHeaders"]),
            reroll: vecs(&s["reroll"]),
            chromatic_header: vecs(&s["chromaticHeader"]),
            gifts: protos(&s["gifts"]),
            prey: s["prey"].as_str().map(unpack),
            tusk: s["tusk"].as_str().map(unpack),
        };
        Templates { board, cells, screens, digits }
    })
}
