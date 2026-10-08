//! Clipboard previews never replace the original format bytes.
use serde::Serialize;
use std::sync::Arc;

pub const MAX_ENTRY_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_HISTORY_BYTES: usize = 128 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Format {
    pub id: u32,
    pub name: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Content {
    pub formats: Vec<Format>,
    pub view: EntryView,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryView {
    pub id: u64,
    pub kind: String,
    pub label: String,
    pub detail: String,
    pub text: Option<String>,
    pub thumbnail: Option<String>,
    pub table: Vec<Vec<String>>,
    pub formats: Vec<String>,
    pub hex: String,
}

impl EntryView {
    pub fn text(text: &str) -> Self {
        Self {
            id: 0,
            kind: "text".into(),
            label: crate::core::preview(text),
            detail: String::new(),
            text: Some(text.into()),
            thumbnail: None,
            table: Vec::new(),
            formats: Vec::new(),
            hex: String::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub view: EntryView,
    pub content: Option<Arc<Content>>,
}

impl Entry {
    pub fn bytes(&self) -> usize {
        self.content.as_ref().map_or_else(
            || self.view.text.as_ref().map_or(0, String::len),
            |content| {
                content
                    .formats
                    .iter()
                    .map(|format| format.bytes.len())
                    .sum()
            },
        )
    }
}

pub fn describe(formats: Vec<Format>) -> Content {
    let text = formats.iter().find(|f| f.id == 13).and_then(|f| {
        if f.bytes.len() > 2 * 1024 * 1024 {
            return None;
        }
        let units: Vec<_> = f
            .bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .take_while(|u| *u != 0)
            .collect();
        String::from_utf16(&units).ok()
    });
    let mut view = EntryView::text(text.as_deref().unwrap_or(""));
    view.text = text.clone();
    let spreadsheet = text.as_ref().is_some_and(|text| text.contains('\t'))
        || (text.is_some()
            && formats.iter().any(|format| {
                format.name.to_ascii_lowercase().starts_with("biff")
                    || format.name == "XML Spreadsheet"
            }));
    view.formats = formats.iter().map(|f| f.name.clone()).collect();
    let bytes: usize = formats.iter().map(|f| f.bytes.len()).sum();
    view.detail = format!("{} KB · {} formats", bytes.div_ceil(1024), formats.len());
    view.hex = formats
        .iter()
        .find(|f| f.id != 13 && f.id != 1 && f.id != 7)
        .or(formats.first())
        .map_or(String::new(), |f| {
            f.bytes
                .iter()
                .take(16)
                .map(|b| format!("{b:02X}"))
                .collect::<Vec<_>>()
                .join(" ")
        });
    if let Some(drop) = formats.iter().find(|f| f.id == 15) {
        let paths = file_paths(&drop.bytes);
        view.kind = "files".into();
        view.text = None;
        let first = paths
            .first()
            .map(|p| p.rsplit(['\\', '/']).next().unwrap_or(p))
            .unwrap_or("Files");
        view.label = if paths.len() > 1 {
            format!("{first} + {} files", paths.len() - 1)
        } else {
            first.into()
        };
        view.detail = format!("{} file(s) · File references", paths.len());
        // Show names and paths in details; never read a copied file just to preview it.
        view.formats.extend(paths);
    } else if let Some(image) = formats.iter().filter(|_| !spreadsheet).find(|f| {
        [
            "png",
            "image/png",
            "jfif",
            "jpeg",
            "image/jpeg",
            "gif",
            "image/gif",
        ]
        .contains(&f.name.to_ascii_lowercase().as_str())
            || f.id == 17
            || f.id == 8
    }) {
        view.kind = "image".into();
        view.text = None;
        if let Some((width, height, thumbnail)) = image_preview(image) {
            view.label = format!("Image · {width} × {height}");
            view.thumbnail = Some(thumbnail);
        } else {
            view.label = "Image".into();
        }
    } else if spreadsheet {
        view.kind = "table".into();
        view.label = "Spreadsheet cells".into();
        view.table = text
            .as_ref()
            .unwrap()
            .lines()
            .take(3)
            .map(|line| {
                line.split('\t')
                    .take(4)
                    .map(|cell| cell.chars().take(40).collect())
                    .collect()
            })
            .collect();
    } else if text.is_none() {
        view.kind = "binary".into();
        view.label = format!("Binary data · {} KB", bytes.div_ceil(1024));
    }
    Content { formats, view }
}

pub fn file_paths(bytes: &[u8]) -> Vec<String> {
    if bytes.len() < 20 {
        return Vec::new();
    }
    let offset = u32::from_le_bytes(bytes[0..4].try_into().unwrap()) as usize;
    if offset < 20 || offset >= bytes.len() {
        return Vec::new();
    }
    if bytes[16..20] == [0, 0, 0, 0] {
        return bytes[offset..]
            .split(|b| *b == 0)
            .take_while(|p| !p.is_empty())
            .take(100)
            .map(|p| String::from_utf8_lossy(p).into_owned())
            .collect();
    }
    let units: Vec<_> = bytes[offset..]
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    units
        .split(|u| *u == 0)
        .take_while(|p| !p.is_empty())
        .take(100)
        .map(String::from_utf16_lossy)
        .collect()
}

fn image_preview(format: &Format) -> Option<(u32, u32, String)> {
    use base64::Engine;
    let (width, height, pixels) = if format.name.eq_ignore_ascii_case("PNG")
        || format.name.eq_ignore_ascii_case("image/png")
    {
        let mut decoder = png::Decoder::new(std::io::Cursor::new(&format.bytes));
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        decoder.set_limits(png::Limits {
            bytes: MAX_ENTRY_BYTES,
        });
        let mut reader = decoder.read_info().ok()?;
        let info = reader.info();
        let (w, h) = (info.width, info.height);
        if w == 0
            || h == 0
            || (w as usize).checked_mul(h as usize)?.checked_mul(4)? > MAX_ENTRY_BYTES
        {
            return None;
        }
        let mut buf = vec![0; reader.output_buffer_size()?];
        let info = reader.next_frame(&mut buf).ok()?;
        let channels = info.color_type.samples();
        let pixels = buf[..info.buffer_size()]
            .chunks_exact(channels)
            .flat_map(|c| match info.color_type {
                png::ColorType::Rgb => [c[0], c[1], c[2], 255],
                png::ColorType::Rgba => [c[0], c[1], c[2], c[3]],
                png::ColorType::Grayscale => [c[0], c[0], c[0], 255],
                png::ColorType::GrayscaleAlpha => [c[0], c[0], c[0], c[1]],
                _ => [0, 0, 0, 255],
            })
            .collect::<Vec<_>>();
        (w, h, pixels)
    } else {
        let b = &format.bytes;
        if b.len() < 40 {
            return None;
        }
        let header = u32::from_le_bytes(b[0..4].try_into().ok()?) as usize;
        let width = i32::from_le_bytes(b[4..8].try_into().ok()?);
        let signed_height = i32::from_le_bytes(b[8..12].try_into().ok()?);
        let bits = u16::from_le_bytes(b[14..16].try_into().ok()?) as usize;
        let compression = u32::from_le_bytes(b[16..20].try_into().ok()?);
        if width <= 0
            || signed_height == 0
            || signed_height == i32::MIN
            || ![24, 32].contains(&bits)
            || !(compression == 0 || (compression == 3 && bits == 32))
        {
            return None;
        }
        let (w, h) = (width as usize, signed_height.unsigned_abs() as usize);
        if w.checked_mul(h)?.checked_mul(4)? > MAX_ENTRY_BYTES {
            return None;
        }
        let stride = w.checked_mul(bits)?.div_ceil(32).checked_mul(4)?;
        let pixel_offset = if compression == 3 && header == 40 {
            header + 12
        } else {
            header
        };
        if header < 40 || pixel_offset.checked_add(stride.checked_mul(h)?)? > b.len() {
            return None;
        }
        let masks = if compression == 3 {
            if b.len() < 52 {
                return None;
            }
            [
                u32::from_le_bytes(b[40..44].try_into().ok()?),
                u32::from_le_bytes(b[44..48].try_into().ok()?),
                u32::from_le_bytes(b[48..52].try_into().ok()?),
                if header >= 56 {
                    u32::from_le_bytes(b[52..56].try_into().ok()?)
                } else {
                    0
                },
            ]
        } else {
            [0x00ff0000, 0x0000ff00, 0x000000ff, 0]
        };
        let mut pixels = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            let source_y = if signed_height > 0 { h - 1 - y } else { y };
            for x in 0..w {
                let i = pixel_offset + source_y * stride + x * (bits / 8);
                if compression == 3 {
                    let pixel = u32::from_le_bytes(b[i..i + 4].try_into().ok()?);
                    for (channel, mask) in masks.iter().enumerate() {
                        let value = if *mask == 0 {
                            if channel == 3 { 255 } else { 0 }
                        } else {
                            let shift = mask.trailing_zeros();
                            (((pixel & mask) >> shift) as u64 * 255 / (mask >> shift) as u64) as u8
                        };
                        pixels.push(value);
                    }
                } else {
                    pixels.extend_from_slice(&[b[i + 2], b[i + 1], b[i], 255]);
                }
            }
        }
        (w as u32, h as u32, pixels)
    };
    let scale = (width.max(height) as f64 / 96.0).max(1.0);
    let (tw, th) = (
        (width as f64 / scale).round().max(1.0) as u32,
        (height as f64 / scale).round().max(1.0) as u32,
    );
    let mut thumbnail = Vec::with_capacity((tw * th * 4) as usize);
    for y in 0..th {
        for x in 0..tw {
            let i = (((y as u64 * height as u64 / th as u64) * width as u64
                + x as u64 * width as u64 / tw as u64)
                * 4) as usize;
            thumbnail.extend_from_slice(&pixels[i..i + 4]);
        }
    }
    let mut png_bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png_bytes, tw, th);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .ok()?
            .write_image_data(&thumbnail)
            .ok()?;
    }
    Some((
        width,
        height,
        format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(png_bytes)
        ),
    ))
}

#[cfg(windows)]
#[path = "content_windows.rs"]
mod native;
#[cfg(windows)]
pub use native::{capture, read_text, restore, write_text};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dib_bitfields_thumbnail_is_a_valid_png_with_original_dimensions() {
        use base64::Engine;
        let mut dib = vec![0; 128];
        dib[..4].copy_from_slice(&124u32.to_le_bytes());
        dib[4..8].copy_from_slice(&1i32.to_le_bytes());
        dib[8..12].copy_from_slice(&(-1i32).to_le_bytes());
        dib[12..14].copy_from_slice(&1u16.to_le_bytes());
        dib[14..16].copy_from_slice(&32u16.to_le_bytes());
        dib[16..20].copy_from_slice(&3u32.to_le_bytes());
        for (offset, mask) in [
            (40, 0xff0000u32),
            (44, 0xff00),
            (48, 0xff),
            (52, 0xff000000),
        ] {
            dib[offset..offset + 4].copy_from_slice(&mask.to_le_bytes());
        }
        dib[124..128].copy_from_slice(&[0x22, 0x44, 0x88, 0xff]);
        let (w, h, url) = image_preview(&Format {
            id: 17,
            name: "DIBV5".into(),
            bytes: dib,
        })
        .unwrap();
        assert_eq!((w, h), (1, 1));
        let encoded = base64::engine::general_purpose::STANDARD
            .decode(url.split_once(',').unwrap().1)
            .unwrap();
        let mut reader = png::Decoder::new(std::io::Cursor::new(encoded))
            .read_info()
            .unwrap();
        let mut pixel = vec![0; reader.output_buffer_size().unwrap()];
        reader.next_frame(&mut pixel).unwrap();
        assert_eq!(pixel, [0x88, 0x44, 0x22, 0xff]);
    }
    #[test]
    fn table_and_binary_previews_keep_original_bytes() {
        let raw: Vec<u8> = "Name\tTotal\r\nTeam\t42\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        let table = describe(vec![Format {
            id: 13,
            name: "Unicode text".into(),
            bytes: raw.clone(),
        }]);
        assert_eq!(table.view.kind, "table");
        assert_eq!(table.view.table[1], ["Team", "42"]);
        assert_eq!(table.formats[0].bytes, raw);
        let binary = describe(vec![Format {
            id: 49152,
            name: "Test binary".into(),
            bytes: vec![0xde, 0xad, 0xbe, 0xef],
        }]);
        assert_eq!(binary.view.kind, "binary");
        assert_eq!(binary.view.hex, "DE AD BE EF");
        assert!(binary.view.text.is_none());
    }
    #[test]
    fn malformed_images_and_file_lists_are_bounded() {
        assert!(file_paths(&[0; 20]).is_empty());
        assert!(
            image_preview(&Format {
                id: 8,
                name: "DIB".into(),
                bytes: vec![0; 40]
            })
            .is_none()
        );
    }
}
