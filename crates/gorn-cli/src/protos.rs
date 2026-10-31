//! Эталоны с кадров доски: места панели орудий (`screens.json`, раздел
//! `tools`) и портреты камер (`cells.json`, раздел `beasts`) — что узнано
//! сейчас и вектор для нового эталона, когда узнать нечем; карты выбора орудий
//! (`screens.json`, `pickArt` и `pickTitles`).

use std::path::Path;

use gorn_prison::recognize::board::{classify_face, read_tier};
use gorn_prison::recognize::cells::{face_vec, prey_vec, FACE_LEN, PREY_LEN};
use gorn_prison::recognize::layout::{chromatic_slot, tool_slot, PICK_CARDS, TOOL_SLOTS};
use gorn_prison::recognize::panels::read_chromatic;
use gorn_prison::recognize::pick::card_debug;
use gorn_prison::recognize::sample::{dot, normalize, sample_into};
use gorn_prison::recognize::templates::{nearest, scores_by_label};
use gorn_prison::recognize::{calibrate, default_geometry, templates};

use crate::frames::{decode_png, pngs};

/// Вектор эталона: «масштаб;base64» из int8, как читает `templates::unpack`.
fn pack(v: &[f32]) -> String {
    let scale = v.iter().fold(0.0f64, |a, &x| a.max((x as f64).abs())).max(1e-9);
    let bytes: Vec<u8> = v.iter().map(|&x| ((x as f64 / scale * 127.0).round() as i8) as u8).collect();
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(bytes.len() * 4 / 3 + 4);
    for ch in bytes.chunks(3) {
        let n = ch.iter().enumerate().fold(0u32, |a, (i, &b)| a | (b as u32) << (16 - 8 * i));
        for i in 0..=ch.len() {
            s.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
        for _ in ch.len()..3 {
            s.push('=');
        }
    }
    format!("{scale:.6};{s}")
}

pub fn proto(label: &str, v: &[f32]) -> String {
    format!("{{\"label\":\"{label}\",\"v\":\"{}\"}}", pack(v))
}

/// Что снимать: место панели орудий или портрет камеры.
#[derive(Clone, Copy)]
pub enum Target {
    Slot(usize),
    Cell(usize),
    /// Все портреты строкой: класс и сходство камер 1…15.
    Cells,
    /// Значок внизу портретов («Добыча», «Бивень») камер 1…15; эталон — камеры N.
    Mark(usize),
    /// Карты выбора орудий: сходство с эталонами картинки и названия; эталон — карты N (0–2).
    Pick(usize),
    /// Места хранилища призматических: сходство с «занято» и «пусто»; эталон — места N (0–9).
    Chromatic(usize),
}

/// Кадры доски из папки (или один PNG) с ЧЧММСС в [from, to]: по строке на
/// кадр — места панели (узнанное, сходство, контраст) или портрет камеры
/// (класс, сходство, редкость по рамке). label — ещё и эталон строкой JSON.
pub fn report(path: &Path, from: &str, to: &str, target: Target, label: Option<&str>) -> Result<(), String> {
    let files = if path.is_dir() { pngs(path) } else { vec![path.to_path_buf()] };
    let t = templates();
    for f in files {
        let name = f.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
        let hms = name.get(..6).unwrap_or_default();
        let want = if matches!(target, Target::Pick(_)) { !name.contains("-board") } else { name.contains("-board") };
        if path.is_dir() && (!want || hms < from || hms > to) {
            continue;
        }
        let img = decode_png(&f)?;
        let frame = img.frame();
        if let Target::Pick(card) = target {
            // Экран без доски — геометрия по размеру кадра, как у распознавания.
            let geo = default_geometry(&frame);
            let mut line = name.clone();
            for (i, &cx) in PICK_CARDS.iter().enumerate() {
                let (rows, art, title) = card_debug(&frame, &geo, cx, &t.screens);
                let top: Vec<String> = rows.iter().take(3).map(|r| format!("{} {:.2}={:.2}+{:.2}", r.0, r.3, r.1, r.2)).collect();
                line += &format!("  [{i}] {}", top.join(" / "));
                if i == card {
                    if let Some(l) = label {
                        println!("pickArt {}", proto(l, &art));
                        println!("pickTitles {}", proto(l, &title));
                    }
                }
            }
            println!("{line}");
            continue;
        }
        let geo = calibrate(&frame, &t.board).unwrap_or_else(|| default_geometry(&frame));
        match target {
            Target::Slot(slot) => {
                let geo = geo.left();
                let mut buf = [0.0f32; 12 * 18 * 3];
                let mut line = name.clone();
                let mut out = None;
                for i in 0..TOOL_SLOTS {
                    sample_into(&frame, &geo, tool_slot(i), 12, 18, 3, &mut buf);
                    let contrast = normalize(&mut buf);
                    let (l, score) = nearest(&buf, &t.screens.tools);
                    line += &format!("  [{i}] {l} {score:.3} к{contrast:.1}");
                    if i == slot {
                        out = label.map(|l| proto(l, &buf));
                    }
                }
                println!("{line}");
                if let Some(p) = out {
                    println!("{p}");
                }
            }
            Target::Cell(cell) => {
                let (l, score) = classify_face(&frame, &geo, cell, &t.cells);
                println!("{name}  камера {cell}: {l} {score:.3} редкость {}", read_tier(&frame, &geo, cell, false));
                if let Some(l) = label {
                    let mut buf = [0.0f32; FACE_LEN];
                    face_vec(&frame, &geo, cell, &mut buf);
                    println!("{}", proto(l, &buf));
                }
            }
            Target::Cells => {
                let mut line = name.clone();
                for cell in 1..16 {
                    let (l, score) = classify_face(&frame, &geo, cell, &t.cells);
                    line += &format!("  [{cell}] {l} {score:.3}");
                }
                println!("{line}");
            }
            Target::Pick(_) => unreachable!(),
            Target::Chromatic(slot) => {
                let geo = geo.right();
                let mut buf = [0.0f32; 12 * 16 * 3];
                let mut line = format!("{name}  хранилище {}", read_chromatic(&frame, &calibrate(&frame, &t.board).unwrap_or_else(|| default_geometry(&frame)), &t.screens));
                for i in 0..10 {
                    sample_into(&frame, &geo, chromatic_slot(i), 12, 16, 3, &mut buf);
                    let contrast = normalize(&mut buf);
                    let sc = scores_by_label(&buf, &t.screens.chromatic);
                    let g = |l: &str| sc.iter().find(|x| x.0 == l).map_or(f64::NAN, |x| x.1);
                    line += &format!("  [{i}] з{:.2} п{:.2} к{contrast:.1}", g("filled"), g("empty"));
                    if i == slot {
                        if let Some(l) = label {
                            println!("{}", proto(l, &buf));
                        }
                    }
                }
                println!("{line}");
            }
            Target::Mark(cell) => {
                let score = |v: &[f32], t: &Option<Vec<f32>>| t.as_ref().map_or(f64::NAN, |t| dot(v, t));
                let mut buf = [0.0f32; PREY_LEN];
                let mut line = name.clone();
                for c in 1..16 {
                    prey_vec(&frame, &geo, c, &mut buf);
                    line += &format!("  [{c}] п{:.2} б{:.2}", score(&buf, &t.screens.prey), score(&buf, &t.screens.tusk));
                }
                println!("{line}");
                if let Some(l) = label {
                    prey_vec(&frame, &geo, cell, &mut buf);
                    println!("{}", proto(l, &buf));
                }
            }
        }
    }
    Ok(())
}
