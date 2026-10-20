//! Кадры тюрьмы с диска: PNG → RGBA (как decodePng эталона) и поиск кадров
//! в папке данных (~/Documents/gorn-prison-data или GORN_PRISON_DATA).

use std::path::{Path, PathBuf};

/// RGBA построчно; альфа для распознавания не нужна.
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
}

impl Image {
    pub fn frame(&self) -> gorn_prison::recognize::Frame<'_> {
        gorn_prison::recognize::Frame { width: self.width, height: self.height, data: &self.data }
    }
}

pub fn decode_png(path: &Path) -> Result<Image, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut dec = png::Decoder::new(std::io::BufReader::new(file));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().map_err(|e| format!("{}: {e}", path.display()))?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| format!("{}: {e}", path.display()))?;
    let (w, h) = (info.width as usize, info.height as usize);
    let src = &buf[..info.buffer_size()];
    let mut data = vec![255u8; w * h * 4];
    use png::ColorType::*;
    match info.color_type {
        Rgba => data.copy_from_slice(&src[..w * h * 4]),
        Rgb => {
            for i in 0..w * h {
                data[i * 4..i * 4 + 3].copy_from_slice(&src[i * 3..i * 3 + 3]);
            }
        }
        Grayscale | GrayscaleAlpha => {
            let ch = if info.color_type == Grayscale { 1 } else { 2 };
            for i in 0..w * h {
                let g = src[i * ch];
                data[i * 4] = g;
                data[i * 4 + 1] = g;
                data[i * 4 + 2] = g;
                if ch == 2 {
                    data[i * 4 + 3] = src[i * 2 + 1];
                }
            }
        }
        Indexed => return Err(format!("{}: палитра не раскрыта", path.display())),
    }
    Ok(Image { width: w, height: h, data })
}

/// Папка данных распознавания.
pub fn data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("GORN_PRISON_DATA") {
        return PathBuf::from(d);
    }
    let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap_or_default();
    Path::new(&home).join("Documents").join("gorn-prison-data")
}

/// Все PNG под папкой, по порядку путей.
pub fn pngs(dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("png")) {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}
