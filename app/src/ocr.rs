//! Названия карт Vorax — встроенным распознаванием текста Windows (Windows.Media.Ocr), как у vorax-overlay
//! (DinoRaos/vorax-solver-rs, MIT, src/app/ocr.rs). Своё: сначала русский (клиент Горна русский), иначе
//! английский, иначе язык профиля. Вызывать из потока с RoInitialize.

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
        for tag in ["ru", "ru-RU", "en-US"] {
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
