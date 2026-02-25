use crate::error::AppError;
use crate::file_collect::{collect_targets_with_extensions, JPEG_ALLOWED_EXTENSIONS};
use crate::fs_atomic::atomic_write_replace;
use crate::model::{
    ExifOffsetExecuteDetail, ExifOffsetExecuteResponse, ExifOffsetPreviewItem,
    ExifOffsetPreviewRequest, ExifOffsetPreviewResponse, ExecuteStatus, OperationProgressEvent,
    PreviewStatus,
};
use chrono::NaiveDateTime;
use exif::{In, Reader, Tag, Value};
use std::fs;
use std::io::BufReader;
use std::path::Path;

/// EXIF datetime tag IDs
const TAG_DATETIME: u16 = 0x0132;
const TAG_DATETIME_ORIGINAL: u16 = 0x9003;
const TAG_DATETIME_DIGITIZED: u16 = 0x9004;
const TAG_EXIF_IFD_POINTER: u16 = 0x8769;

/// Read the primary EXIF datetime string from a JPEG file.
/// Priority: DateTimeOriginal > DateTimeDigitized > DateTime
fn read_exif_datetime_string(path: &Path) -> Option<String> {
    let file = fs::File::open(path).ok()?;
    let mut reader = BufReader::new(file);
    let exif = Reader::new().read_from_container(&mut reader).ok()?;

    let tags = [Tag::DateTimeOriginal, Tag::DateTimeDigitized, Tag::DateTime];
    for tag in tags {
        if let Some(field) = exif.get_field(tag, In::PRIMARY) {
            if let Value::Ascii(ref vec) = field.value {
                if !vec.is_empty() {
                    if let Ok(s) = String::from_utf8(vec[0].clone()) {
                        let trimmed = s.trim().trim_matches('\0');
                        if !trimmed.is_empty() {
                            return Some(trimmed.to_string());
                        }
                    }
                }
            }
        }
    }
    None
}

/// Apply offset_seconds to an EXIF datetime string "YYYY:MM:DD HH:MM:SS"
fn apply_offset(datetime_str: &str, offset_seconds: i64) -> Option<String> {
    let naive = NaiveDateTime::parse_from_str(datetime_str, "%Y:%m:%d %H:%M:%S").ok()?;
    let adjusted = naive
        .checked_add_signed(chrono::Duration::seconds(offset_seconds))?;
    Some(adjusted.format("%Y:%m:%d %H:%M:%S").to_string())
}

pub fn preview(request: &ExifOffsetPreviewRequest) -> Result<ExifOffsetPreviewResponse, AppError> {
    let collect = collect_targets_with_extensions(
        &request.input_paths,
        request.include_subfolders,
        JPEG_ALLOWED_EXTENSIONS,
    )
    .map_err(AppError::InvalidRequest)?;

    if collect.files.is_empty() {
        let msg = if collect.skipped_by_extension > 0 {
            format!(
                "対応していないファイル形式です（{}件のファイルが拡張子でスキップされました）",
                collect.skipped_by_extension
            )
        } else {
            "対象ファイルが見つかりません。".to_string()
        };
        return Err(AppError::InvalidRequest(msg));
    }

    let mut items = Vec::with_capacity(collect.files.len());
    let mut ready = 0usize;
    let mut skipped = 0usize;

    for file in &collect.files {
        let path_str = file.to_string_lossy().to_string();
        match read_exif_datetime_string(file) {
            Some(original) => {
                match apply_offset(&original, request.offset_seconds) {
                    Some(corrected) => {
                        ready += 1;
                        items.push(ExifOffsetPreviewItem {
                            source_path: path_str,
                            original_datetime: Some(original),
                            corrected_datetime: Some(corrected),
                            status: PreviewStatus::Ready,
                            reason: None,
                        });
                    }
                    None => {
                        skipped += 1;
                        items.push(ExifOffsetPreviewItem {
                            source_path: path_str,
                            original_datetime: Some(original),
                            corrected_datetime: None,
                            status: PreviewStatus::Skipped,
                            reason: Some("オフセット適用後の日時が範囲外です".to_string()),
                        });
                    }
                }
            }
            None => {
                skipped += 1;
                items.push(ExifOffsetPreviewItem {
                    source_path: path_str,
                    original_datetime: None,
                    corrected_datetime: None,
                    status: PreviewStatus::Skipped,
                    reason: Some("EXIF日時情報がありません".to_string()),
                });
            }
        }
    }

    Ok(ExifOffsetPreviewResponse {
        total: ready + skipped,
        ready,
        skipped,
        items,
    })
}

pub fn execute<FCancel, FProgress>(
    request: &ExifOffsetPreviewRequest,
    is_cancelled: FCancel,
    mut report_progress: FProgress,
) -> Result<ExifOffsetExecuteResponse, AppError>
where
    FCancel: Fn() -> bool,
    FProgress: FnMut(OperationProgressEvent),
{
    let collect = collect_targets_with_extensions(
        &request.input_paths,
        request.include_subfolders,
        JPEG_ALLOWED_EXTENSIONS,
    )
    .map_err(AppError::InvalidRequest)?;

    let total = collect.files.len();
    let mut details = Vec::with_capacity(total);
    let mut succeeded = 0usize;
    let mut failed = 0usize;
    let mut skipped = 0usize;
    let mut processed = 0usize;
    let mut canceled = false;

    for file in &collect.files {
        if !canceled && is_cancelled() {
            canceled = true;
        }

        let path_str = file.to_string_lossy().to_string();

        if canceled {
            skipped += 1;
            processed += 1;
            details.push(ExifOffsetExecuteDetail {
                source_path: path_str.clone(),
                status: ExecuteStatus::Skipped,
                reason: Some("キャンセルされました".to_string()),
            });
            report_progress(OperationProgressEvent {
                operation: "exifOffset".to_string(),
                processed,
                total,
                succeeded,
                failed,
                skipped,
                current_path: Some(path_str),
                done: false,
                canceled,
            });
            continue;
        }

        // Check EXIF datetime exists
        let original = match read_exif_datetime_string(file) {
            Some(dt) => dt,
            None => {
                skipped += 1;
                processed += 1;
                details.push(ExifOffsetExecuteDetail {
                    source_path: path_str.clone(),
                    status: ExecuteStatus::Skipped,
                    reason: Some("EXIF日時情報がありません".to_string()),
                });
                report_progress(OperationProgressEvent {
                    operation: "exifOffset".to_string(),
                    processed,
                    total,
                    succeeded,
                    failed,
                    skipped,
                    current_path: Some(path_str),
                    done: false,
                    canceled,
                });
                continue;
            }
        };

        let corrected = match apply_offset(&original, request.offset_seconds) {
            Some(c) => c,
            None => {
                skipped += 1;
                processed += 1;
                details.push(ExifOffsetExecuteDetail {
                    source_path: path_str.clone(),
                    status: ExecuteStatus::Skipped,
                    reason: Some("オフセット適用後の日時が範囲外です".to_string()),
                });
                report_progress(OperationProgressEvent {
                    operation: "exifOffset".to_string(),
                    processed,
                    total,
                    succeeded,
                    failed,
                    skipped,
                    current_path: Some(path_str),
                    done: false,
                    canceled,
                });
                continue;
            }
        };

        match modify_exif_dates(file, request.offset_seconds) {
            Ok(_) => {
                succeeded += 1;
                details.push(ExifOffsetExecuteDetail {
                    source_path: path_str.clone(),
                    status: ExecuteStatus::Succeeded,
                    reason: Some(format!("{} → {}", original, corrected)),
                });
            }
            Err(e) => {
                failed += 1;
                details.push(ExifOffsetExecuteDetail {
                    source_path: path_str.clone(),
                    status: ExecuteStatus::Failed,
                    reason: Some(e),
                });
            }
        }

        processed += 1;
        report_progress(OperationProgressEvent {
            operation: "exifOffset".to_string(),
            processed,
            total,
            succeeded,
            failed,
            skipped,
            current_path: Some(path_str),
            done: false,
            canceled,
        });
    }

    report_progress(OperationProgressEvent {
        operation: "exifOffset".to_string(),
        processed,
        total,
        succeeded,
        failed,
        skipped,
        current_path: None,
        done: true,
        canceled,
    });

    Ok(ExifOffsetExecuteResponse {
        processed: succeeded + failed + skipped,
        succeeded,
        failed,
        skipped,
        details,
    })
}

/// Modify EXIF datetime fields in a JPEG file by binary patching.
///
/// EXIF datetime fields are fixed-length ASCII "YYYY:MM:DD HH:MM:SS\0" (20 bytes).
/// We overwrite them in-place with the offset-adjusted value.
fn modify_exif_dates(path: &Path, offset_seconds: i64) -> Result<(), String> {
    let data = fs::read(path)
        .map_err(|e| format!("ファイルの読み込みに失敗しました: {}", e))?;

    if data.len() < 4 || data[0] != 0xFF || data[1] != 0xD8 {
        return Err("JPEGファイルではありません".to_string());
    }

    let mut modified = data.clone();
    let mut any_modified = false;

    // Walk JPEG markers to find APP1 (0xFFE1)
    let mut pos = 2usize;
    while pos + 4 <= modified.len() {
        if modified[pos] != 0xFF {
            break;
        }
        let marker = modified[pos + 1];

        // End markers
        if marker == 0xDA || marker == 0xD9 {
            break;
        }

        // Markers without length (standalone markers like RST, SOI, etc.)
        if marker == 0x00 || (0xD0..=0xD7).contains(&marker) {
            pos += 2;
            continue;
        }

        if pos + 4 > modified.len() {
            break;
        }

        let seg_len =
            ((modified[pos + 2] as usize) << 8) | (modified[pos + 3] as usize);
        if seg_len < 2 || pos + 2 + seg_len > modified.len() {
            break;
        }

        if marker == 0xE1 {
            // APP1 segment found
            let seg_start = pos + 4; // after marker (2) + length (2)
            let seg_end = pos + 2 + seg_len;

            if seg_end > modified.len() {
                break;
            }

            // Check "Exif\0\0" header
            if seg_start + 6 <= seg_end
                && &modified[seg_start..seg_start + 6] == b"Exif\0\0"
            {
                let tiff_start = seg_start + 6;
                if let Ok(changed) =
                    patch_exif_dates(&mut modified, tiff_start, seg_end, offset_seconds)
                {
                    if changed {
                        any_modified = true;
                    }
                }
            }
        }

        pos += 2 + seg_len;
    }

    if !any_modified {
        return Err("書き換え可能なEXIF日時フィールドが見つかりません".to_string());
    }

    atomic_write_replace(path, &modified)?;
    Ok(())
}

/// Detect byte order from TIFF header
#[derive(Clone, Copy, PartialEq)]
enum ByteOrder {
    Little,
    Big,
}

fn read_u16(data: &[u8], offset: usize, order: ByteOrder) -> u16 {
    match order {
        ByteOrder::Little => u16::from_le_bytes([data[offset], data[offset + 1]]),
        ByteOrder::Big => u16::from_be_bytes([data[offset], data[offset + 1]]),
    }
}

fn read_u32(data: &[u8], offset: usize, order: ByteOrder) -> u32 {
    match order {
        ByteOrder::Little => u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]),
        ByteOrder::Big => u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]),
    }
}

/// Patch all EXIF datetime fields within the TIFF data.
fn patch_exif_dates(
    data: &mut Vec<u8>,
    tiff_start: usize,
    seg_end: usize,
    offset_seconds: i64,
) -> Result<bool, String> {
    if tiff_start + 8 > seg_end {
        return Err("TIFF header too short".to_string());
    }

    let order = match &data[tiff_start..tiff_start + 2] {
        b"II" => ByteOrder::Little,
        b"MM" => ByteOrder::Big,
        _ => return Err("Unknown byte order".to_string()),
    };

    // Verify TIFF magic 42
    let magic = read_u16(data, tiff_start + 2, order);
    if magic != 42 {
        return Err("Invalid TIFF magic".to_string());
    }

    let ifd0_offset = read_u32(data, tiff_start + 4, order) as usize;
    let ifd0_abs = tiff_start + ifd0_offset;

    let mut changed = false;
    let mut exif_ifd_offset: Option<usize> = None;

    // Walk IFD0
    if ifd0_abs + 2 <= seg_end {
        let entry_count = read_u16(data, ifd0_abs, order) as usize;
        for i in 0..entry_count {
            let entry_abs = ifd0_abs + 2 + i * 12;
            if entry_abs + 12 > seg_end {
                break;
            }
            let tag = read_u16(data, entry_abs, order);
            let dtype = read_u16(data, entry_abs + 2, order);
            let count = read_u32(data, entry_abs + 4, order) as usize;

            if tag == TAG_DATETIME && dtype == 2 && count == 20 {
                let value_offset = read_u32(data, entry_abs + 8, order) as usize;
                let abs_offset = tiff_start + value_offset;
                if abs_offset + 20 <= seg_end {
                    if patch_datetime_at(data, abs_offset, offset_seconds) {
                        changed = true;
                    }
                }
            }

            if tag == TAG_EXIF_IFD_POINTER {
                let sub_offset = read_u32(data, entry_abs + 8, order) as usize;
                exif_ifd_offset = Some(tiff_start + sub_offset);
            }
        }
    }

    // Walk Exif IFD
    if let Some(exif_ifd_abs) = exif_ifd_offset {
        if exif_ifd_abs + 2 <= seg_end {
            let entry_count = read_u16(data, exif_ifd_abs, order) as usize;
            for i in 0..entry_count {
                let entry_abs = exif_ifd_abs + 2 + i * 12;
                if entry_abs + 12 > seg_end {
                    break;
                }
                let tag = read_u16(data, entry_abs, order);
                let dtype = read_u16(data, entry_abs + 2, order);
                let count = read_u32(data, entry_abs + 4, order) as usize;

                if (tag == TAG_DATETIME_ORIGINAL || tag == TAG_DATETIME_DIGITIZED)
                    && dtype == 2
                    && count == 20
                {
                    let value_offset = read_u32(data, entry_abs + 8, order) as usize;
                    let abs_offset = tiff_start + value_offset;
                    if abs_offset + 20 <= seg_end {
                        if patch_datetime_at(data, abs_offset, offset_seconds) {
                            changed = true;
                        }
                    }
                }
            }
        }
    }

    Ok(changed)
}

/// Patch a single datetime field at the given byte offset.
/// Returns true if the field was successfully patched.
fn patch_datetime_at(data: &mut Vec<u8>, offset: usize, offset_seconds: i64) -> bool {
    if offset + 20 > data.len() {
        return false;
    }

    // Read current datetime string (19 bytes + null terminator)
    let datetime_bytes = &data[offset..offset + 19];
    let datetime_str = match std::str::from_utf8(datetime_bytes) {
        Ok(s) => s.trim(),
        Err(_) => return false,
    };

    if datetime_str.is_empty() || datetime_str.chars().all(|c| c == '\0' || c == ' ') {
        return false;
    }

    let adjusted = match apply_offset(datetime_str, offset_seconds) {
        Some(s) => s,
        None => return false,
    };

    // Write back as 19 bytes + null terminator
    let adjusted_bytes = adjusted.as_bytes();
    if adjusted_bytes.len() != 19 {
        return false;
    }

    data[offset..offset + 19].copy_from_slice(adjusted_bytes);
    data[offset + 19] = 0; // null terminator

    true
}
