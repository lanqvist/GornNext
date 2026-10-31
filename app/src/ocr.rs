//! Названия карт Vorax — встроенным распознаванием текста Windows (Windows.Media.Ocr), как у vorax-overlay
//! (DinoRaos/vorax-solver-rs, MIT, src/app/ocr.rs). Своё: язык клиента игры по логу (`client.rs`) —
//! английский клиент читается английским, иначе сначала русский; нет нужного — язык профиля. Вызывать из
//! потока с RoInitialize.

use gorn_vorax_scan::pixels::Img;
use windows::core::HSTRING;
use windows::Globalization::Language;
use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;

pub struct Ocr {
    engine: OcrEngine,
    /// язык распознавания: "ru", "en-US", …
    pub language: String,
}

impl Ocr {
    pub fn new() -> windows::core::Result<Ocr> {
        let mut engine = None;
        let tags: &[&str] = if crate::client::english() == Some(true) {
            &["en-US", "en-GB", "en"]
        } else {
            &["ru", "ru-RU", "en-US"]
        };
        for &tag in tags {
            let lang = Language::CreateLanguage(&HSTRING::from(tag))?;
            if OcrEngine::IsLanguageSupported(&lang)? {
                engine = Some(OcrEngine::TryCreateFromLanguage(&lang)?);
                break;
            }
        }
        let engine = match engine {
            Some(e) => e,
            None => OcrEngine::TryCreateFromUserProfileLanguages()?,
        };
        let language = engine.RecognizerLanguage()?.LanguageTag()?.to_string();
        Ok(Ocr { engine, language })
    }

    pub fn russian(&self) -> bool {
        self.language.starts_with("ru")
    }

    /// Текст картинки (тёмный по белому), строка за строкой.
    pub fn read(&self, img: &Img) -> windows::core::Result<String> {
        let bmp = SoftwareBitmap::CreateWithAlpha(
            BitmapPixelFormat::Bgra8,
            img.width as i32,
            img.height as i32,
            BitmapAlphaMode::Premultiplied,
        )?;
        let mut bgra = img.data.clone();
        for p in bgra.chunks_exact_mut(4) {
            p.swap(0, 2);
        }
        let writer = DataWriter::new()?;
        writer.WriteBytes(&bgra)?;
        bmp.CopyFromBuffer(&writer.DetachBuffer()?)?;
        let result = self.engine.RecognizeAsync(&bmp)?.join()?;
        let mut lines = Vec::new();
        for line in result.Lines()? {
            lines.push(line.Text()?.to_string());
        }
        Ok(lines.join("\n"))
    }
}

/// Светлый текст на тёмном фоне из прямоугольника кадра (RGBA, пиксели кадра)
/// → тёмный текст на белом для OCR: яркость над местным средним, как у
/// названий карт Vorax (`gorn_vorax_scan::titles::title_crop`).
pub fn light_text(data: &[u8], width: usize, height: usize, b: [f64; 4]) -> Img {
    const BLUR: usize = 6;
    let x0 = (b[0].max(0.0) as usize).min(width.saturating_sub(1));
    let y0 = (b[1].max(0.0) as usize).min(height.saturating_sub(1));
    let x1 = (b[2].max(0.0) as usize).clamp(x0 + 1, width);
    let y1 = (b[3].max(0.0) as usize).clamp(y0 + 1, height);
    let (w, h) = (x1 - x0, y1 - y0);
    let g: Vec<f64> = (0..w * h)
        .map(|i| {
            let k = ((y0 + i / w) * width + x0 + i % w) * 4;
            data[k].max(data[k + 1]).max(data[k + 2]) as f64
        })
        .collect();
    let mut sum = vec![0f64; (w + 1) * (h + 1)];
    for y in 0..h {
        for x in 0..w {
            sum[(y + 1) * (w + 1) + x + 1] = g[y * w + x] + sum[y * (w + 1) + x + 1] + sum[(y + 1) * (w + 1) + x] - sum[y * (w + 1) + x];
        }
    }
    let mut lift = vec![0f64; w * h];
    let mut top = 1.0f64;
    for y in 0..h {
        for x in 0..w {
            let (a0, a1, b0, b1) = (x.saturating_sub(BLUR), w.min(x + BLUR + 1), y.saturating_sub(BLUR), h.min(y + BLUR + 1));
            let mean = (sum[b1 * (w + 1) + a1] - sum[b0 * (w + 1) + a1] - sum[b1 * (w + 1) + a0] + sum[b0 * (w + 1) + a0]) / ((a1 - a0) * (b1 - b0)) as f64;
            lift[y * w + x] = (g[y * w + x] - mean).max(0.0);
            top = top.max(lift[y * w + x]);
        }
    }
    let mut out = Img::new(w, h);
    for i in 0..w * h {
        let v = (255.0 - (lift[i] / top * 400.0).min(255.0)).round() as u8;
        out.data[i * 4..i * 4 + 4].copy_from_slice(&[v, v, v, 255]);
    }
    out
}
