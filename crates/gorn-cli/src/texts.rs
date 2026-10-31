//! Эталоны надписей (`screens.json`: `headers`, `objectives`, `pickHeaders`,
//! `reroll`, `chromaticHeader`) — для клиента на другом языке: что прочитано
//! сейчас (лучшая метка и сходство) и вектор нового эталона с `--label`.
//! Картинки (значки, портреты, иконки орудий) от языка не зависят — их
//! снимает `protos`.

use std::path::Path;

use gorn_prison::recognize::layout::{CHROMATIC_HEADER, HEADER, HEADER_H, HEADER_W, OBJECTIVE, PICK_HEADER, PICK_REROLL};
use gorn_prison::recognize::sample::{dot, normalize, sample_into, Box4, Frame, Geometry};
use gorn_prison::recognize::templates::nearest;
use gorn_prison::recognize::{calibrate, default_geometry, read, templates, ReadOptions};

use crate::frames::{decode_png, pngs};
use crate::protos::proto;

/// Надпись: раздел screens.json, где на экране, размер выборки, с какой стороны якорь.
struct Spot {
    key: &'static str,
    b: Box4,
    w: usize,
    h: usize,
    side: Side,
}

#[derive(Clone, Copy)]
enum Side {
    Center,
    Left,
    Right,
}

const SPOTS: [Spot; 5] = [
    Spot { key: "headers", b: HEADER, w: HEADER_W, h: HEADER_H, side: Side::Center },
    Spot { key: "objectives", b: OBJECTIVE, w: 80, h: 15, side: Side::Left },
    Spot { key: "chromaticHeader", b: CHROMATIC_HEADER, w: 92, h: 20, side: Side::Right },
    Spot { key: "pickHeaders", b: PICK_HEADER, w: 100, h: 6, side: Side::Center },
    Spot { key: "reroll", b: PICK_REROLL, w: 44, h: 10, side: Side::Center },
];

fn vector(frame: &Frame, geo: &Geometry, s: &Spot) -> (Vec<f32>, f64) {
    let geo = match s.side {
        Side::Center => *geo,
        Side::Left => geo.left(),
        Side::Right => geo.right(),
    };
    let mut buf = vec![0.0f32; s.w * s.h];
    sample_into(frame, &geo, s.b, s.w, s.h, 1, &mut buf);
    let contrast = normalize(&mut buf);
    (buf, contrast)
}

/// Кадры (папка или PNG): по строке на кадр — экран по распознаванию и по
/// каждой надписи «метка сходство». `what` + `label` — ещё и эталон этой
/// надписи строкой JSON (раздел screens.json и запись).
pub fn report(path: &Path, what: Option<&str>, label: Option<&str>) -> Result<(), String> {
    if let Some(w) = what {
        if !SPOTS.iter().any(|s| s.key == w) {
            return Err(format!("texts --what: одно из {}", SPOTS.map(|s| s.key).join(", ")));
        }
    }
    let files = if path.is_dir() { pngs(path) } else { vec![path.to_path_buf()] };
    let t = templates();
    let s = &t.screens;
    for f in files {
        let name = f.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
        let img = decode_png(&f)?;
        let frame = img.frame();
        let (view, found) = read(&frame, None, ReadOptions { recalibrate: true });
        let geo = found.or_else(|| calibrate(&frame, &t.board)).unwrap_or_else(|| default_geometry(&frame));
        let kind = format!("{view:?}");
        let kind = kind.split(['(', ' ', '{']).next().unwrap_or_default().to_string();
        let mut line = format!("{name} {kind}");
        if let gorn_prison::view::ScreenView::Board(b) = &view {
            let cards: Vec<String> = gorn_prison::recognize::cards::card_centers(&frame, &geo)
                .iter()
                .map(|&cx| {
                    let (l, sc) = gorn_prison::recognize::cards::classify_card(&frame, &geo, cx, &t.screens.cards);
                    format!("{cx:.0}:{l} {sc:.2}")
                })
                .collect();
            line += &format!(" карточки {cards:?}");
            if std::env::var("DOORS").is_ok() {
                for &cx in &gorn_prison::recognize::cards::card_centers(&frame, &geo) {
                    println!("  дверь {cx:.0}: {}", gorn_prison::recognize::cards::door_debug(&frame, &geo, cx, &t.digits));
                }
            }
            line += &format!(" [надзиратель {:?}, двери {:?}, решение {:?}, цепочка {:?}, дни {:?}, цель {:?}, призм {}]", b.warden, b.doors, b.decision, b.chain, b.days_left, b.objective, b.chromatic);
        }
        for spot in &SPOTS {
            let (v, contrast) = vector(&frame, &geo, spot);
            let read = match spot.key {
                "headers" => Some(nearest(&v, &s.headers)),
                "objectives" => Some(nearest(&v, &s.objectives)),
                "pickHeaders" => Some(nearest(&v, &s.pick_headers)),
                _ => None,
            };
            let plain = |list: &[Vec<f32>]| list.iter().map(|p| dot(&v, p)).fold(-2.0f64, f64::max);
            let (l, sc) = match (spot.key, read) {
                (_, Some((l, sc))) => (l.to_string(), sc),
                ("reroll", _) => ("·".into(), plain(&s.reroll)),
                _ => ("·".into(), plain(&s.chromatic_header)),
            };
            line += &format!("  {}={l} {sc:.2} (к{contrast:.0})", spot.key);
            if what == Some(spot.key) {
                if let Some(lb) = label {
                    match spot.key {
                        // В этих разделах — голые векторы, без метки.
                        "reroll" | "chromaticHeader" => println!("{} \"{}\"", spot.key, packed(&v)),
                        _ => println!("{} {}", spot.key, proto(lb, &v)),
                    }
                }
            }
        }
        println!("{line}");
    }
    Ok(())
}

fn packed(v: &[f32]) -> String {
    let p = proto("", v);
    // {"label":"","v":"…"} → …
    p.rsplit_once("\"v\":\"").map(|(_, r)| r.trim_end_matches("\"}").to_string()).unwrap_or(p)
}
