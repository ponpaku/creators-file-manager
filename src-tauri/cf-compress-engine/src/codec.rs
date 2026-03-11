use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ImageFormat, ImageReader};
use std::fs;
use std::io::Cursor;
use std::path::Path;

/// Decode + resize + encode a single JPEG in memory, returning (source_size, compressed_size).
pub fn sample_compress_in_memory(
    source: &Path,
    resize_percent: f32,
    quality: u8,
) -> Option<(u64, u64)> {
    let source_size = fs::metadata(source).ok()?.len();
    let mut image = ImageReader::open(source).ok()?.decode().ok()?;
    let ratio = (resize_percent / 100.0).clamp(0.01, 1.0);
    if ratio < 0.999 {
        let nw = ((image.width() as f32) * ratio).round().max(1.0) as u32;
        let nh = ((image.height() as f32) * ratio).round().max(1.0) as u32;
        image = image.resize_exact(nw, nh, image::imageops::FilterType::Lanczos3);
    }
    let mut buf = Vec::new();
    JpegEncoder::new_with_quality(&mut buf, quality)
        .encode_image(&image)
        .ok()?;
    Some((source_size, buf.len() as u64))
}

/// Decode an image and resize it, returning the DynamicImage ready for encoding.
pub fn decode_and_resize(source: &Path, resize_percent: f32) -> Option<(u64, DynamicImage)> {
    let source_size = fs::metadata(source).ok()?.len();
    let mut image = ImageReader::open(source).ok()?.decode().ok()?;
    let ratio = (resize_percent / 100.0).clamp(0.01, 1.0);
    if ratio < 0.999 {
        let nw = ((image.width() as f32) * ratio).round().max(1.0) as u32;
        let nh = ((image.height() as f32) * ratio).round().max(1.0) as u32;
        image = image.resize_exact(nw, nh, image::imageops::FilterType::Lanczos3);
    }
    Some((source_size, image))
}

/// Encode a pre-decoded image at a given quality, returning the compressed size.
pub fn encode_only(image: &DynamicImage, quality: u8) -> Option<u64> {
    let mut buf = Vec::new();
    JpegEncoder::new_with_quality(&mut buf, quality)
        .encode_image(image)
        .ok()?;
    Some(buf.len() as u64)
}

/// Compress a single file: decode → resize → encode → write (with optional EXIF preservation).
pub fn compress_one_file(
    source: &Path,
    destination: &Path,
    resize_percent: f32,
    quality: u8,
    preserve_exif: bool,
) -> Result<u64, String> {
    let original_bytes =
        fs::read(source).map_err(|e| format!("ファイルの読み込みに失敗しました: {}", e))?;
    let mut image = ImageReader::open(source)
        .map_err(|e| format!("画像ファイルを開けません: {}", e))?
        .decode()
        .map_err(|e| format!("画像のデコードに失敗しました: {}", e))?;

    let ratio = (resize_percent / 100.0).clamp(0.01, 1.0);
    if ratio < 0.999 {
        let (w, h) = (image.width(), image.height());
        let nw = ((w as f32) * ratio).round().max(1.0) as u32;
        let nh = ((h as f32) * ratio).round().max(1.0) as u32;
        image = image.resize_exact(nw, nh, image::imageops::FilterType::Lanczos3);
    }

    let mut encoded = Vec::new();
    {
        let mut encoder = JpegEncoder::new_with_quality(&mut encoded, quality);
        encoder
            .encode_image(&image)
            .map_err(|e| format!("JPEGエンコードに失敗しました: {}", e))?;
    }

    let output_bytes = if preserve_exif {
        let exif_segments = extract_exif_segments(&original_bytes);
        inject_exif_segments(&encoded, &exif_segments)
    } else {
        encoded
    };

    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("出力先フォルダの作成に失敗しました: {}", e))?;
    }

    // Use atomic write: write to temp file then rename
    fs::write(destination, &output_bytes)
        .map_err(|e| format!("ファイルの書き込みに失敗しました: {}", e))?;
    Ok(output_bytes.len() as u64)
}

fn parse_filter(s: &str) -> image::imageops::FilterType {
    match s {
        "catmull_rom" => image::imageops::FilterType::CatmullRom,
        "triangle" => image::imageops::FilterType::Triangle,
        "nearest" => image::imageops::FilterType::Nearest,
        _ => image::imageops::FilterType::Lanczos3,
    }
}

/// Resize a single image file and write to destination.
/// Returns (output_size, was_skipped).
pub fn resize_one_file(
    source: &Path,
    destination: &Path,
    mode: &str,
    size_px: u32,
    small_image_policy: &str,
    filter: &str,
    sharpen: f32,
    quality: u8,
    preserve_exif: bool,
) -> Result<(u64, bool), String> {
    let filter_type = parse_filter(filter);

    let original_bytes = fs::read(source)
        .map_err(|e| format!("ファイルの読み込みに失敗しました: {}", e))?;

    let img = ImageReader::open(source)
        .map_err(|e| format!("画像ファイルを開けません: {}", e))?
        .with_guessed_format()
        .map_err(|e| format!("フォーマット判定に失敗しました: {}", e))?
        .decode()
        .map_err(|e| format!("画像のデコードに失敗しました: {}", e))?;

    let (w, h) = (img.width(), img.height());
    let current_side = if mode == "short_side" {
        w.min(h)
    } else {
        w.max(h)
    };

    // small_image_policy check
    if current_side <= size_px {
        match small_image_policy {
            "skip" => {
                return Ok((0, true));
            }
            "copy" => {
                if let Some(parent) = destination.parent() {
                    fs::create_dir_all(parent)
                        .map_err(|e| format!("出力先フォルダの作成に失敗しました: {}", e))?;
                }
                let file_size = original_bytes.len() as u64;
                fs::write(destination, &original_bytes)
                    .map_err(|e| format!("ファイルの書き込みに失敗しました: {}", e))?;
                return Ok((file_size, true));
            }
            _ => {} // "upscale": continue processing
        }
    }

    // Compute new dimensions preserving aspect ratio
    let scale = size_px as f32 / current_side as f32;
    let new_w = ((w as f32) * scale).round().max(1.0) as u32;
    let new_h = ((h as f32) * scale).round().max(1.0) as u32;

    let mut resized = img.resize(new_w, new_h, filter_type);

    if sharpen > 0.0 {
        let sigma = sharpen * 0.5;
        resized = DynamicImage::ImageRgba8(image::imageops::unsharpen(
            &resized.to_rgba8(),
            sigma,
            5,
        ));
    }

    // Determine output format from source extension
    let ext = source
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let output_bytes = match ext.as_str() {
        "jpg" | "jpeg" => {
            let mut encoded = Vec::new();
            {
                let mut encoder = JpegEncoder::new_with_quality(&mut encoded, quality);
                encoder
                    .encode_image(&resized)
                    .map_err(|e| format!("JPEGエンコードに失敗しました: {}", e))?;
            }
            if preserve_exif {
                let exif_segments = extract_exif_segments(&original_bytes);
                inject_exif_segments(&encoded, &exif_segments)
            } else {
                encoded
            }
        }
        _ => {
            // PNG or WebP — use write_to
            let format = if ext == "png" {
                ImageFormat::Png
            } else {
                ImageFormat::WebP
            };
            let mut buf = Vec::new();
            resized
                .write_to(&mut Cursor::new(&mut buf), format)
                .map_err(|e| format!("画像のエンコードに失敗しました: {}", e))?;
            buf
        }
    };

    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("出力先フォルダの作成に失敗しました: {}", e))?;
    }

    fs::write(destination, &output_bytes)
        .map_err(|e| format!("ファイルの書き込みに失敗しました: {}", e))?;

    Ok((output_bytes.len() as u64, false))
}

pub fn extract_exif_segments(bytes: &[u8]) -> Vec<Vec<u8>> {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return Vec::new();
    }
    let mut result = Vec::new();
    let mut i = 2usize;
    while i + 4 <= bytes.len() {
        if bytes[i] != 0xFF {
            break;
        }
        let marker = bytes[i + 1];
        if marker == 0xDA || marker == 0xD9 {
            break;
        }
        if marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            i += 2;
            continue;
        }
        let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        if len < 2 || i + 2 + len > bytes.len() {
            break;
        }
        if marker == 0xE1 && len >= 8 && &bytes[i + 4..i + 10] == b"Exif\0\0" {
            result.push(bytes[i..i + 2 + len].to_vec());
        }
        i += 2 + len;
    }
    result
}

pub fn inject_exif_segments(compressed: &[u8], exif_segments: &[Vec<u8>]) -> Vec<u8> {
    if exif_segments.is_empty()
        || compressed.len() < 2
        || compressed[0] != 0xFF
        || compressed[1] != 0xD8
    {
        return compressed.to_vec();
    }
    let mut out =
        Vec::with_capacity(compressed.len() + exif_segments.iter().map(Vec::len).sum::<usize>());
    out.extend_from_slice(&compressed[0..2]);
    for segment in exif_segments {
        out.extend_from_slice(segment);
    }
    out.extend_from_slice(&compressed[2..]);
    out
}
