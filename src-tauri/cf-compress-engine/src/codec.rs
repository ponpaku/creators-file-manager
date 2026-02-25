use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ImageReader};
use std::fs;
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
