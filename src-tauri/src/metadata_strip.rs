use crate::error::AppError;
use crate::file_collect::{collect_targets_with_extensions, JPEG_ALLOWED_EXTENSIONS};
use crate::fs_atomic::atomic_write_replace;
use crate::model::{
    ExecuteStatus, MetadataStripCategories, MetadataStripExecuteDetail,
    MetadataStripExecuteResponse, MetadataStripPreviewItem, MetadataStripPreviewRequest,
    MetadataStripPreviewResponse, OperationProgressEvent, PreviewStatus,
};
use std::fs;
use std::path::Path;

// ===== TIFF Byte Order =====

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

fn write_u16(buf: &mut Vec<u8>, v: u16, order: ByteOrder) {
    match order {
        ByteOrder::Little => buf.extend_from_slice(&v.to_le_bytes()),
        ByteOrder::Big => buf.extend_from_slice(&v.to_be_bytes()),
    }
}

fn write_u32(buf: &mut Vec<u8>, v: u32, order: ByteOrder) {
    match order {
        ByteOrder::Little => buf.extend_from_slice(&v.to_le_bytes()),
        ByteOrder::Big => buf.extend_from_slice(&v.to_be_bytes()),
    }
}

fn patch_u32(buf: &mut [u8], pos: usize, value: u32, order: ByteOrder) {
    let bytes = match order {
        ByteOrder::Little => value.to_le_bytes(),
        ByteOrder::Big => value.to_be_bytes(),
    };
    buf[pos..pos + 4].copy_from_slice(&bytes);
}

fn read_u32_inline(data: &[u8], order: ByteOrder) -> u32 {
    if data.len() < 4 {
        return 0;
    }
    match order {
        ByteOrder::Little => {
            u32::from_le_bytes([data[0], data[1], data[2], data[3]])
        }
        ByteOrder::Big => {
            u32::from_be_bytes([data[0], data[1], data[2], data[3]])
        }
    }
}

// ===== IFD Entry =====

fn type_byte_size(dtype: u16) -> usize {
    match dtype {
        1 | 2 | 6 | 7 => 1,  // BYTE, ASCII, SBYTE, UNDEFINED
        3 | 8 => 2,           // SHORT, SSHORT
        4 | 9 | 11 => 4,      // LONG, SLONG, FLOAT
        5 | 10 | 12 => 8,     // RATIONAL, SRATIONAL, DOUBLE
        _ => 1,
    }
}

#[derive(Clone)]
struct IfdEntry {
    tag: u16,
    dtype: u16,
    count: u32,
    data: Vec<u8>, // actual data bytes (inline or de-referenced overflow)
}

impl IfdEntry {
    fn byte_count(&self) -> usize {
        type_byte_size(self.dtype) * self.count as usize
    }
}

// ===== Tag constants =====

// IFD pointer tags
const TAG_EXIF_IFD_POINTER: u16 = 0x8769;
const TAG_GPS_IFD_POINTER: u16 = 0x8825;

// IFD0 tags by category
const IFD0_CAMERA_LENS_TAGS: &[u16] = &[0x010F, 0x0110]; // Make, Model
const IFD0_SOFTWARE_TAGS: &[u16] = &[0x0131, 0x013C, 0x000B]; // Software, HostComputer, ProcessingSoftware
const IFD0_AUTHOR_COPYRIGHT_TAGS: &[u16] = &[0x013B, 0x8298]; // Artist, Copyright
const IFD0_COMMENT_TAGS: &[u16] = &[0x010E]; // ImageDescription

// Exif IFD tags by category
const EXIF_CAMERA_LENS_TAGS: &[u16] = &[0xA433, 0xA434, 0xA431, 0xA435, 0xA432]; // LensMake, LensModel, BodySerialNumber, LensSerialNumber, LensInfo
const EXIF_SOFTWARE_TAGS: &[u16] = &[0x000B]; // ProcessingSoftware
const EXIF_AUTHOR_COPYRIGHT_TAGS: &[u16] = &[0x9C9D]; // XPAuthor
const EXIF_COMMENT_TAGS: &[u16] = &[0x9286, 0x9C9C, 0x9C9B, 0x9C9E, 0x9C9F]; // UserComment, XPComment, XPTitle, XPSubject, XPKeywords
const EXIF_MAKER_NOTE_TAG: u16 = 0x927C;

// Shooting settings tags (Exif IFD)
const EXIF_SHOOTING_SETTINGS_TAGS: &[u16] = &[
    0x829A, 0x829D, // ExposureTime, FNumber
    0x8822, // ExposureProgram
    0x8827, // ISOSpeedRatings
    0x9201, 0x9202, 0x9203, 0x9204, 0x9205, 0x9206, // ShutterSpeedValue, ApertureValue, BrightnessValue, ExposureBiasValue, MaxApertureValue, SubjectDistance
    0x9207, 0x9208, 0x9209, // MeteringMode, LightSource, Flash
    0x920A, // FocalLength
    0xA20E, 0xA20F, 0xA210, // FocalPlaneXResolution, FocalPlaneYResolution, FocalPlaneResolutionUnit
    0xA215, // ExposureIndex
    0xA217, // SensingMethod
    0xA300, 0xA301, // FileSource, SceneType
    0xA302, // CFAPattern
    0xA401, 0xA402, 0xA403, 0xA404, 0xA405, 0xA406, // CustomRendered, ExposureMode, WhiteBalance, DigitalZoomRatio, FocalLengthIn35mmFilm, SceneCaptureType
    0xA407, 0xA408, 0xA409, 0xA40A, 0xA40B, 0xA40C, // GainControl, Contrast, Saturation, Sharpness, DeviceSettingDescription, SubjectDistanceRange
    0xA420, // ImageUniqueID
    0x8830, 0x8831, 0x8832, 0x8833, 0x8834, 0x8835, // SensitivityType, StandardOutputSensitivity, RecommendedExposureIndex, ISOSpeed, ISOSpeedLatitudeyyy, ISOSpeedLatitudezzz
    0xA460, 0xA461, 0xA462, // CompositeImage, SourceImageNumberOfCompositeImage, SourceExposureTimesOfCompositeImage
];

// Capture datetime tags (IFD0 and Exif IFD)
const IFD0_DATETIME_TAG: u16 = 0x0132; // DateTime
const EXIF_DATETIME_TAGS: &[u16] = &[
    0x9003, 0x9004, // DateTimeOriginal, DateTimeDigitized
    0x9290, 0x9291, 0x9292, // SubSecTime, SubSecTimeOriginal, SubSecTimeDigitized
];

// Thumbnail tags in IFD1
const TAG_JPEG_INTERCHANGE_FORMAT: u16 = 0x0201;
const TAG_JPEG_INTERCHANGE_FORMAT_LENGTH: u16 = 0x0202;

fn should_remove_ifd0_tag(tag: u16, cats: &MetadataStripCategories, is_full_clean: bool) -> bool {
    // GPS pointer
    if tag == TAG_GPS_IFD_POINTER && cats.gps {
        return true;
    }
    // Exif pointer: never remove here (handled separately)
    if tag == TAG_EXIF_IFD_POINTER {
        return false;
    }
    if cats.camera_lens && IFD0_CAMERA_LENS_TAGS.contains(&tag) {
        return true;
    }
    if cats.software && IFD0_SOFTWARE_TAGS.contains(&tag) {
        return true;
    }
    if cats.author_copyright && IFD0_AUTHOR_COPYRIGHT_TAGS.contains(&tag) {
        return true;
    }
    if cats.comments && IFD0_COMMENT_TAGS.contains(&tag) {
        return true;
    }
    // capture_datetime: IFD0 DateTime
    if tag == IFD0_DATETIME_TAG && cats.capture_datetime {
        return true;
    }
    // In full clean mode, remove all non-essential tags not listed above
    if is_full_clean {
        // Keep only truly essential tags
        const ESSENTIAL_IFD0: &[u16] = &[
            0x0100, 0x0101, // ImageWidth, ImageLength
            0x0102, 0x0103, 0x0106, // BitsPerSample, Compression, PhotometricInterpretation
            0x011A, 0x011B, 0x0128, // XResolution, YResolution, ResolutionUnit
            0x0112, // Orientation
            0x0115, // SamplesPerPixel
            0x0213, // YCbCrPositioning
            0x0211, 0x0212, // YCbCrCoefficients, YCbCrSubSampling
            0x013E, 0x013F, 0x0142, 0x0143, // WhitePoint, PrimaryChromaticities, HalfToneHints, TileWidth
            TAG_EXIF_IFD_POINTER,
        ];
        return !ESSENTIAL_IFD0.contains(&tag);
    }
    false
}

fn should_remove_exif_tag(tag: u16, cats: &MetadataStripCategories, is_full_clean: bool) -> bool {
    // Always preserve image dimension/color tags
    const ALWAYS_KEEP_EXIF: &[u16] = &[
        0xA002, 0xA003, // PixelXDimension, PixelYDimension
        0xA001, // ColorSpace
    ];
    if ALWAYS_KEEP_EXIF.contains(&tag) {
        return false;
    }

    // capture_datetime: DateTimeOriginal, DateTimeDigitized, SubSec tags
    if EXIF_DATETIME_TAGS.contains(&tag) {
        return cats.capture_datetime || is_full_clean;
    }

    // shooting_settings
    if EXIF_SHOOTING_SETTINGS_TAGS.contains(&tag) {
        return cats.shooting_settings || is_full_clean;
    }

    // MakerNote: remove only in full clean
    if tag == EXIF_MAKER_NOTE_TAG {
        return is_full_clean;
    }
    if cats.camera_lens && EXIF_CAMERA_LENS_TAGS.contains(&tag) {
        return true;
    }
    if cats.software && EXIF_SOFTWARE_TAGS.contains(&tag) {
        return true;
    }
    if cats.author_copyright && EXIF_AUTHOR_COPYRIGHT_TAGS.contains(&tag) {
        return true;
    }
    if cats.comments && EXIF_COMMENT_TAGS.contains(&tag) {
        return true;
    }
    // Full clean: remove everything not in keep list
    if is_full_clean {
        return true;
    }
    false
}

// ===== IFD Parsing =====

fn parse_ifd_entries(
    data: &[u8],
    tiff_start: usize,
    ifd_rel_offset: usize, // offset from tiff_start
    seg_end: usize,
    order: ByteOrder,
) -> (Vec<IfdEntry>, u32) {
    let ifd_abs = tiff_start + ifd_rel_offset;
    if ifd_abs + 2 > seg_end {
        return (vec![], 0);
    }
    let entry_count = read_u16(data, ifd_abs, order) as usize;
    let mut entries = Vec::with_capacity(entry_count);

    for i in 0..entry_count {
        let entry_abs = ifd_abs + 2 + i * 12;
        if entry_abs + 12 > seg_end {
            break;
        }
        let tag = read_u16(data, entry_abs, order);
        let dtype = read_u16(data, entry_abs + 2, order);
        let count = read_u32(data, entry_abs + 4, order);
        let byte_count = type_byte_size(dtype) * count as usize;

        let entry_data = if byte_count == 0 {
            vec![]
        } else if byte_count <= 4 {
            // Inline value
            let end = (entry_abs + 8 + byte_count).min(seg_end);
            if entry_abs + 8 <= seg_end {
                data[entry_abs + 8..end].to_vec()
            } else {
                vec![]
            }
        } else {
            // Offset-based
            let offset = read_u32(data, entry_abs + 8, order) as usize;
            let abs = tiff_start + offset;
            if abs + byte_count <= seg_end {
                data[abs..abs + byte_count].to_vec()
            } else {
                vec![]
            }
        };

        entries.push(IfdEntry {
            tag,
            dtype,
            count,
            data: entry_data,
        });
    }

    // Read next IFD offset
    let next_ptr_abs = ifd_abs + 2 + entry_count * 12;
    let next_ifd = if next_ptr_abs + 4 <= seg_end {
        read_u32(data, next_ptr_abs, order)
    } else {
        0
    };

    (entries, next_ifd)
}

// ===== Scan metadata =====

struct ScanResult {
    found_gps: bool,
    found_camera_lens: bool,
    found_software: bool,
    found_author_copyright: bool,
    found_comments: bool,
    found_thumbnail: bool,
    found_shooting_settings: bool,
    found_capture_datetime: bool,
    has_iptc: bool,
    has_xmp: bool,
    total_removable_tags: usize, // rough count of tags that could be stripped
    no_exif: bool,
}

fn scan_jpeg_metadata(path: &Path) -> Result<ScanResult, String> {
    let data = fs::read(path).map_err(|e| format!("読み込みエラー: {}", e))?;

    if data.len() < 4 || data[0] != 0xFF || data[1] != 0xD8 {
        return Err("JPEGファイルではありません".to_string());
    }

    let mut result = ScanResult {
        found_gps: false,
        found_camera_lens: false,
        found_software: false,
        found_author_copyright: false,
        found_comments: false,
        found_thumbnail: false,
        found_shooting_settings: false,
        found_capture_datetime: false,
        has_iptc: false,
        has_xmp: false,
        total_removable_tags: 0,
        no_exif: true,
    };

    // Walk JPEG markers
    let mut pos = 2usize;
    while pos + 4 <= data.len() {
        if data[pos] != 0xFF {
            break;
        }
        let marker = data[pos + 1];
        if marker == 0xDA || marker == 0xD9 {
            break;
        }
        if marker == 0x00 || (0xD0..=0xD7).contains(&marker) {
            pos += 2;
            continue;
        }
        if pos + 4 > data.len() {
            break;
        }
        let seg_len = ((data[pos + 2] as usize) << 8) | (data[pos + 3] as usize);
        if seg_len < 2 || pos + 2 + seg_len > data.len() {
            break;
        }
        let seg_start = pos + 4; // after marker(2) + length(2)
        let seg_end = pos + 2 + seg_len;

        match marker {
            0xE1 => {
                // APP1
                if seg_start + 6 <= seg_end
                    && &data[seg_start..seg_start + 6] == b"Exif\0\0"
                {
                    // Exif APP1
                    result.no_exif = false;
                    let tiff_start = seg_start + 6;
                    scan_tiff(&data, tiff_start, seg_end, &mut result);
                } else if seg_start + 29 <= seg_end
                    && &data[seg_start..seg_start + 29] == b"http://ns.adobe.com/xap/1.0/\0"
                {
                    // XMP APP1
                    result.has_xmp = true;
                }
            }
            0xED => {
                // APP13 (IPTC)
                if seg_start + 14 <= seg_end
                    && &data[seg_start..seg_start + 14] == b"Photoshop 3.0\0"
                {
                    result.has_iptc = true;
                }
            }
            _ => {}
        }

        pos += 2 + seg_len;
    }

    Ok(result)
}

fn scan_tiff(data: &[u8], tiff_start: usize, seg_end: usize, result: &mut ScanResult) {
    if tiff_start + 8 > seg_end {
        return;
    }
    let order = match &data[tiff_start..tiff_start + 2] {
        b"II" => ByteOrder::Little,
        b"MM" => ByteOrder::Big,
        _ => return,
    };
    let magic = read_u16(data, tiff_start + 2, order);
    if magic != 42 {
        return;
    }
    let ifd0_offset = read_u32(data, tiff_start + 4, order) as usize;
    let (ifd0_entries, ifd0_next) =
        parse_ifd_entries(data, tiff_start, ifd0_offset, seg_end, order);

    let mut exif_ifd_offset: Option<usize> = None;

    for entry in &ifd0_entries {
        match entry.tag {
            TAG_GPS_IFD_POINTER => {
                result.found_gps = true;
                result.total_removable_tags += 1;
            }
            0x010F | 0x0110 => {
                // Make, Model
                result.found_camera_lens = true;
                result.total_removable_tags += 1;
            }
            0x0131 | 0x013C | 0x000B => {
                // Software, HostComputer, ProcessingSoftware
                result.found_software = true;
                result.total_removable_tags += 1;
            }
            0x013B | 0x8298 => {
                // Artist, Copyright
                result.found_author_copyright = true;
                result.total_removable_tags += 1;
            }
            0x010E => {
                // ImageDescription
                result.found_comments = true;
                result.total_removable_tags += 1;
            }
            IFD0_DATETIME_TAG => {
                // DateTime
                result.found_capture_datetime = true;
                result.total_removable_tags += 1;
            }
            TAG_EXIF_IFD_POINTER => {
                exif_ifd_offset =
                    Some(read_u32_inline(&entry.data, order) as usize);
            }
            _ => {}
        }
    }

    // IFD1 (thumbnail)
    if ifd0_next != 0 {
        let (ifd1_entries, _) =
            parse_ifd_entries(data, tiff_start, ifd0_next as usize, seg_end, order);
        if !ifd1_entries.is_empty() {
            result.found_thumbnail = true;
            result.total_removable_tags += ifd1_entries.len();
        }
    }

    // Exif IFD
    if let Some(offset) = exif_ifd_offset {
        let (exif_entries, _) =
            parse_ifd_entries(data, tiff_start, offset, seg_end, order);
        for entry in &exif_entries {
            match entry.tag {
                0xA433 | 0xA434 | 0xA431 | 0xA435 | 0xA432 => {
                    result.found_camera_lens = true;
                    result.total_removable_tags += 1;
                }
                0x9C9D => {
                    result.found_author_copyright = true;
                    result.total_removable_tags += 1;
                }
                0x9286 | 0x9C9C | 0x9C9B | 0x9C9E | 0x9C9F => {
                    result.found_comments = true;
                    result.total_removable_tags += 1;
                }
                EXIF_MAKER_NOTE_TAG => {
                    // Not counted unless full clean
                }
                t if EXIF_DATETIME_TAGS.contains(&t) => {
                    result.found_capture_datetime = true;
                    result.total_removable_tags += 1;
                }
                t if EXIF_SHOOTING_SETTINGS_TAGS.contains(&t) => {
                    result.found_shooting_settings = true;
                    result.total_removable_tags += 1;
                }
                _ => {}
            }
        }
    }
}

// ===== TIFF Rebuild =====

struct StripResult {
    new_tiff: Vec<u8>,
    stripped_count: usize,
}

fn rebuild_tiff(
    data: &[u8],
    tiff_start: usize,
    seg_end: usize,
    order: ByteOrder,
    cats: &MetadataStripCategories,
    is_full_clean: bool,
) -> Result<StripResult, String> {
    if tiff_start + 8 > seg_end {
        return Err("TIFFヘッダーが短すぎます".to_string());
    }

    let ifd0_rel = read_u32(data, tiff_start + 4, order) as usize;
    let (ifd0_entries, ifd0_next_rel) =
        parse_ifd_entries(data, tiff_start, ifd0_rel, seg_end, order);

    // Find Exif IFD offset
    let exif_ifd_rel: Option<usize> = ifd0_entries
        .iter()
        .find(|e| e.tag == TAG_EXIF_IFD_POINTER)
        .and_then(|e| {
            if e.data.len() >= 4 {
                Some(read_u32_inline(&e.data, order) as usize)
            } else {
                None
            }
        });

    // Parse Exif IFD
    let (exif_entries, _) = if let Some(offset) = exif_ifd_rel {
        parse_ifd_entries(data, tiff_start, offset, seg_end, order)
    } else {
        (vec![], 0)
    };

    // Parse IFD1 (thumbnail)
    let (ifd1_entries, _) = if ifd0_next_rel != 0 {
        parse_ifd_entries(data, tiff_start, ifd0_next_rel as usize, seg_end, order)
    } else {
        (vec![], 0)
    };

    // Extract thumbnail JPEG data if keeping thumbnail
    let thumbnail_data: Option<Vec<u8>> = if !cats.thumbnail && !ifd1_entries.is_empty() {
        let jpeg_offset_entry = ifd1_entries.iter().find(|e| e.tag == TAG_JPEG_INTERCHANGE_FORMAT);
        let jpeg_length_entry = ifd1_entries
            .iter()
            .find(|e| e.tag == TAG_JPEG_INTERCHANGE_FORMAT_LENGTH);
        if let (Some(fmt), Some(len)) = (jpeg_offset_entry, jpeg_length_entry) {
            let offset = read_u32_inline(&fmt.data, order) as usize;
            let length = read_u32_inline(&len.data, order) as usize;
            let abs = tiff_start + offset;
            if length > 0 && abs + length <= seg_end {
                Some(data[abs..abs + length].to_vec())
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    // Filter Exif IFD entries
    let filtered_exif: Vec<IfdEntry> = exif_entries
        .iter()
        .filter(|e| !should_remove_exif_tag(e.tag, cats, is_full_clean))
        .cloned()
        .collect();

    let has_exif = !filtered_exif.is_empty();

    // Filter IFD0 entries
    let filtered_ifd0: Vec<IfdEntry> = ifd0_entries
        .iter()
        .filter(|e| {
            if e.tag == TAG_EXIF_IFD_POINTER {
                return has_exif; // Keep pointer only if Exif IFD has entries
            }
            !should_remove_ifd0_tag(e.tag, cats, is_full_clean)
        })
        .cloned()
        .collect();

    // Count stripped tags
    let ifd0_stripped = ifd0_entries.len() - filtered_ifd0.len();
    let exif_stripped = exif_entries.len() - filtered_exif.len();
    let thumb_stripped = if cats.thumbnail { ifd1_entries.len() } else { 0 };
    let stripped_count = ifd0_stripped + exif_stripped + thumb_stripped;

    // ===== Build new TIFF =====
    let mut out: Vec<u8> = Vec::new();

    // TIFF header: byte order + magic(42) + IFD0 offset(8)
    match order {
        ByteOrder::Little => out.extend_from_slice(b"II"),
        ByteOrder::Big => out.extend_from_slice(b"MM"),
    }
    write_u16(&mut out, 42, order);
    write_u32(&mut out, 8, order); // IFD0 at offset 8

    // ===== IFD0 =====
    // We need to patch: Exif IFD pointer value, next_ifd offset
    // Strategy: write entries, record positions of placeholders, patch after

    let ifd0_data_area_start = out.len() + 2 + filtered_ifd0.len() * 12 + 4;
    write_u16(&mut out, filtered_ifd0.len() as u16, order);

    let mut exif_ptr_field_pos: Option<usize> = None;
    let mut ifd0_overflow_cursor = ifd0_data_area_start;

    for entry in &filtered_ifd0 {
        write_u16(&mut out, entry.tag, order);
        write_u16(&mut out, entry.dtype, order);
        write_u32(&mut out, entry.count, order);

        if entry.tag == TAG_EXIF_IFD_POINTER {
            exif_ptr_field_pos = Some(out.len());
            write_u32(&mut out, 0u32, order); // placeholder
        } else {
            let bc = entry.byte_count();
            if bc <= 4 {
                out.extend_from_slice(&entry.data);
                for _ in entry.data.len()..4 {
                    out.push(0);
                }
            } else {
                write_u32(&mut out, ifd0_overflow_cursor as u32, order);
                ifd0_overflow_cursor += bc;
                if ifd0_overflow_cursor % 2 != 0 {
                    ifd0_overflow_cursor += 1;
                }
            }
        }
    }

    // IFD0 next_ifd placeholder
    let ifd0_next_field_pos = out.len();
    write_u32(&mut out, 0u32, order); // placeholder

    // Write IFD0 overflow data
    for entry in &filtered_ifd0 {
        if entry.tag == TAG_EXIF_IFD_POINTER {
            continue;
        }
        let bc = entry.byte_count();
        if bc > 4 {
            out.extend_from_slice(&entry.data);
            if out.len() % 2 != 0 {
                out.push(0);
            }
        }
    }

    // ===== Exif IFD =====
    if has_exif {
        let exif_ifd_pos = out.len() as u32;

        // Patch IFD0 Exif pointer
        if let Some(pos) = exif_ptr_field_pos {
            patch_u32(&mut out, pos, exif_ifd_pos, order);
        }

        let exif_data_area_start = out.len() + 2 + filtered_exif.len() * 12 + 4;
        write_u16(&mut out, filtered_exif.len() as u16, order);

        let mut exif_overflow_cursor = exif_data_area_start;

        for entry in &filtered_exif {
            write_u16(&mut out, entry.tag, order);
            write_u16(&mut out, entry.dtype, order);
            write_u32(&mut out, entry.count, order);

            let bc = entry.byte_count();
            if bc <= 4 {
                out.extend_from_slice(&entry.data);
                for _ in entry.data.len()..4 {
                    out.push(0);
                }
            } else {
                write_u32(&mut out, exif_overflow_cursor as u32, order);
                exif_overflow_cursor += bc;
                if exif_overflow_cursor % 2 != 0 {
                    exif_overflow_cursor += 1;
                }
            }
        }

        // Exif next_ifd = 0
        write_u32(&mut out, 0u32, order);

        // Exif overflow data
        for entry in &filtered_exif {
            let bc = entry.byte_count();
            if bc > 4 {
                out.extend_from_slice(&entry.data);
                if out.len() % 2 != 0 {
                    out.push(0);
                }
            }
        }
    }

    // ===== IFD1 (thumbnail) =====
    if thumbnail_data.is_some() {
        let ifd1_pos = out.len() as u32;
        patch_u32(&mut out, ifd0_next_field_pos, ifd1_pos, order);

        // Filter IFD1: keep all entries, but special-handle JPEGInterchangeFormat pointer
        let filtered_ifd1: Vec<IfdEntry> = ifd1_entries.iter().cloned().collect();
        let ifd1_data_area_start = out.len() + 2 + filtered_ifd1.len() * 12 + 4;
        write_u16(&mut out, filtered_ifd1.len() as u16, order);

        let mut jpeg_ptr_field_pos: Option<usize> = None;
        let mut ifd1_overflow_cursor = ifd1_data_area_start;

        for entry in &filtered_ifd1 {
            write_u16(&mut out, entry.tag, order);
            write_u16(&mut out, entry.dtype, order);
            write_u32(&mut out, entry.count, order);

            if entry.tag == TAG_JPEG_INTERCHANGE_FORMAT {
                jpeg_ptr_field_pos = Some(out.len());
                write_u32(&mut out, 0u32, order); // placeholder
            } else {
                let bc = entry.byte_count();
                if bc <= 4 {
                    out.extend_from_slice(&entry.data);
                    for _ in entry.data.len()..4 {
                        out.push(0);
                    }
                } else {
                    write_u32(&mut out, ifd1_overflow_cursor as u32, order);
                    ifd1_overflow_cursor += bc;
                    if ifd1_overflow_cursor % 2 != 0 {
                        ifd1_overflow_cursor += 1;
                    }
                }
            }
        }

        // IFD1 next_ifd = 0
        write_u32(&mut out, 0u32, order);

        // IFD1 overflow data (excluding thumbnail pointer)
        for entry in &filtered_ifd1 {
            if entry.tag == TAG_JPEG_INTERCHANGE_FORMAT {
                continue;
            }
            let bc = entry.byte_count();
            if bc > 4 {
                out.extend_from_slice(&entry.data);
                if out.len() % 2 != 0 {
                    out.push(0);
                }
            }
        }

        // Write thumbnail JPEG data, patch pointer
        if let Some(ptr_pos) = jpeg_ptr_field_pos {
            let thumb_start = out.len() as u32;
            patch_u32(&mut out, ptr_pos, thumb_start, order);
        }
        out.extend_from_slice(thumbnail_data.as_ref().unwrap());
    }

    Ok(StripResult {
        new_tiff: out,
        stripped_count,
    })
}

// ===== JPEG Segment Processing =====

fn strip_metadata_from_jpeg(
    path: &Path,
    cats: &MetadataStripCategories,
    is_full_clean: bool,
) -> Result<(usize, bool, bool), String> {
    // Returns (stripped_tag_count, stripped_iptc, stripped_xmp)
    let data = fs::read(path).map_err(|e| format!("読み込みエラー: {}", e))?;

    if data.len() < 4 || data[0] != 0xFF || data[1] != 0xD8 {
        return Err("JPEGファイルではありません".to_string());
    }

    let mut out: Vec<u8> = Vec::with_capacity(data.len());
    // SOI
    out.push(0xFF);
    out.push(0xD8);

    let mut stripped_tags = 0usize;
    let mut stripped_iptc = false;
    let mut stripped_xmp = false;

    let mut pos = 2usize;
    while pos < data.len() {
        if data[pos] != 0xFF {
            // Remaining data (image bitstream without SOS marker)
            out.extend_from_slice(&data[pos..]);
            break;
        }
        if pos + 1 >= data.len() {
            // Truncated JPEG: trailing 0xFF with no marker byte — pass through and stop
            out.push(0xFF);
            break;
        }
        let marker = data[pos + 1];

        // SOI / EOI / standalone markers
        if marker == 0xD8 {
            // Duplicate SOI? Keep it.
            out.push(0xFF);
            out.push(marker);
            pos += 2;
            continue;
        }
        if marker == 0xD9 {
            out.push(0xFF);
            out.push(0xD9);
            out.extend_from_slice(&data[pos + 2..]);
            break;
        }
        if marker == 0xDA {
            // SOS: copy everything from here to end
            out.extend_from_slice(&data[pos..]);
            break;
        }
        if marker == 0x00 || (0xD0..=0xD7).contains(&marker) {
            out.push(0xFF);
            out.push(marker);
            pos += 2;
            continue;
        }

        if pos + 4 > data.len() {
            out.extend_from_slice(&data[pos..]);
            break;
        }

        let seg_len = ((data[pos + 2] as usize) << 8) | (data[pos + 3] as usize);
        if seg_len < 2 || pos + 2 + seg_len > data.len() {
            // Malformed, copy remaining as-is
            out.extend_from_slice(&data[pos..]);
            break;
        }

        let seg_payload_start = pos + 4; // payload after marker(2) + length(2)
        let seg_end = pos + 2 + seg_len;

        match marker {
            0xE1 => {
                // APP1
                if seg_payload_start + 6 <= seg_end
                    && &data[seg_payload_start..seg_payload_start + 6] == b"Exif\0\0"
                {
                    // Exif APP1: rebuild TIFF
                    let tiff_start = seg_payload_start + 6;
                    if tiff_start + 8 <= seg_end {
                        let order = match &data[tiff_start..tiff_start + 2] {
                            b"II" => ByteOrder::Little,
                            b"MM" => ByteOrder::Big,
                            _ => {
                                // Unknown byte order, keep as-is
                                out.extend_from_slice(&data[pos..seg_end]);
                                pos = seg_end;
                                continue;
                            }
                        };

                        match rebuild_tiff(&data, tiff_start, seg_end, order, cats, is_full_clean) {
                            Ok(strip_result) => {
                                stripped_tags += strip_result.stripped_count;
                                // Build new APP1 segment: "Exif\0\0" + new TIFF
                                let new_payload_len = 6 + strip_result.new_tiff.len();
                                let new_seg_len = new_payload_len + 2; // +2 for length field itself
                                out.push(0xFF);
                                out.push(0xE1);
                                out.push(((new_seg_len >> 8) & 0xFF) as u8);
                                out.push((new_seg_len & 0xFF) as u8);
                                out.extend_from_slice(b"Exif\0\0");
                                out.extend_from_slice(&strip_result.new_tiff);
                            }
                            Err(_) => {
                                // On error, keep original segment
                                out.extend_from_slice(&data[pos..seg_end]);
                            }
                        }
                    } else {
                        out.extend_from_slice(&data[pos..seg_end]);
                    }
                } else if seg_payload_start + 29 <= seg_end
                    && &data[seg_payload_start..seg_payload_start + 29]
                        == b"http://ns.adobe.com/xap/1.0/\0"
                {
                    // XMP APP1
                    if cats.xmp {
                        stripped_xmp = true;
                        // Remove: don't copy
                    } else {
                        out.extend_from_slice(&data[pos..seg_end]);
                    }
                } else {
                    // Other APP1, keep as-is
                    out.extend_from_slice(&data[pos..seg_end]);
                }
            }
            0xED => {
                // APP13
                if seg_payload_start + 14 <= seg_end
                    && &data[seg_payload_start..seg_payload_start + 14] == b"Photoshop 3.0\0"
                {
                    // IPTC
                    if cats.iptc {
                        stripped_iptc = true;
                        // Remove: don't copy
                    } else {
                        out.extend_from_slice(&data[pos..seg_end]);
                    }
                } else {
                    out.extend_from_slice(&data[pos..seg_end]);
                }
            }
            _ => {
                // All other segments: keep as-is
                out.extend_from_slice(&data[pos..seg_end]);
            }
        }

        pos = seg_end;
    }

    if stripped_tags == 0 && !stripped_iptc && !stripped_xmp {
        return Err("削除するメタデータが見つかりませんでした".to_string());
    }

    atomic_write_replace(path, &out)?;
    Ok((stripped_tags, stripped_iptc, stripped_xmp))
}

// ===== Preset resolution =====

fn preset_to_categories(
    preset: &crate::model::MetadataStripPreset,
    custom: &MetadataStripCategories,
) -> MetadataStripCategories {
    use crate::model::MetadataStripPreset::*;
    match preset {
        SnsPublish => MetadataStripCategories {
            gps: true,
            camera_lens: true,
            software: false,
            author_copyright: false,
            comments: true,
            thumbnail: true,
            iptc: false,
            xmp: false,
            shooting_settings: false,
            capture_datetime: false,
        },
        Delivery => MetadataStripCategories {
            gps: false,
            camera_lens: true,
            software: true,
            author_copyright: false,
            comments: true,
            thumbnail: false,
            iptc: false,
            xmp: false,
            shooting_settings: false,
            capture_datetime: false,
        },
        FullClean => MetadataStripCategories {
            gps: true,
            camera_lens: true,
            software: true,
            author_copyright: true,
            comments: true,
            thumbnail: true,
            iptc: true,
            xmp: true,
            shooting_settings: true,
            capture_datetime: true,
        },
        Custom => custom.clone(),
    }
}

fn is_full_clean_preset(preset: &crate::model::MetadataStripPreset) -> bool {
    matches!(preset, crate::model::MetadataStripPreset::FullClean)
}

// ===== Preview =====

pub fn preview(
    request: &MetadataStripPreviewRequest,
) -> Result<MetadataStripPreviewResponse, AppError> {
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

    let cats = preset_to_categories(&request.preset, &request.categories);

    let mut items = Vec::with_capacity(collect.files.len());
    let mut ready = 0usize;
    let mut skipped = 0usize;

    for file in &collect.files {
        let path_str = file.to_string_lossy().to_string();

        match scan_jpeg_metadata(file) {
            Ok(scan) => {
                if scan.no_exif && !scan.has_iptc && !scan.has_xmp {
                    skipped += 1;
                    items.push(MetadataStripPreviewItem {
                        source_path: path_str,
                        found_categories: vec![],
                        tags_to_strip: 0,
                        has_iptc: false,
                        has_xmp: false,
                        status: PreviewStatus::Skipped,
                        reason: Some("メタデータがありません".to_string()),
                    });
                    continue;
                }

                // Determine what would be stripped given current categories
                let mut found_categories = Vec::new();

                if cats.gps && scan.found_gps {
                    found_categories.push("GPS/位置情報".to_string());
                }
                if cats.camera_lens && scan.found_camera_lens {
                    found_categories.push("カメラ/レンズ情報".to_string());
                }
                if cats.software && scan.found_software {
                    found_categories.push("作成ソフト/編集履歴".to_string());
                }
                if cats.author_copyright && scan.found_author_copyright {
                    found_categories.push("作者/著作権".to_string());
                }
                if cats.comments && scan.found_comments {
                    found_categories.push("コメント/説明".to_string());
                }
                if cats.thumbnail && scan.found_thumbnail {
                    found_categories.push("サムネイル(IFD1)".to_string());
                }
                if cats.iptc && scan.has_iptc {
                    found_categories.push("IPTC(APP13)".to_string());
                }
                if cats.xmp && scan.has_xmp {
                    found_categories.push("XMP(APP1)".to_string());
                }
                if cats.shooting_settings && scan.found_shooting_settings {
                    found_categories.push("撮影時設定".to_string());
                }
                if cats.capture_datetime && scan.found_capture_datetime {
                    found_categories.push("撮影日時".to_string());
                }

                let tags_to_strip = scan.total_removable_tags;

                if found_categories.is_empty() && !scan.has_iptc && !scan.has_xmp {
                    skipped += 1;
                    items.push(MetadataStripPreviewItem {
                        source_path: path_str,
                        found_categories: vec![],
                        tags_to_strip: 0,
                        has_iptc: scan.has_iptc,
                        has_xmp: scan.has_xmp,
                        status: PreviewStatus::Skipped,
                        reason: Some("削除対象のメタデータがありません".to_string()),
                    });
                } else {
                    ready += 1;
                    items.push(MetadataStripPreviewItem {
                        source_path: path_str,
                        found_categories,
                        tags_to_strip,
                        has_iptc: scan.has_iptc,
                        has_xmp: scan.has_xmp,
                        status: PreviewStatus::Ready,
                        reason: None,
                    });
                }
            }
            Err(e) => {
                skipped += 1;
                items.push(MetadataStripPreviewItem {
                    source_path: path_str,
                    found_categories: vec![],
                    tags_to_strip: 0,
                    has_iptc: false,
                    has_xmp: false,
                    status: PreviewStatus::Skipped,
                    reason: Some(e),
                });
            }
        }
    }

    Ok(MetadataStripPreviewResponse {
        total: ready + skipped,
        ready,
        skipped,
        items,
    })
}

// ===== Execute =====

pub fn execute<FCancel, FProgress>(
    request: &MetadataStripPreviewRequest,
    is_cancelled: FCancel,
    mut report_progress: FProgress,
) -> Result<MetadataStripExecuteResponse, AppError>
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

    let cats = preset_to_categories(&request.preset, &request.categories);
    let is_full_clean = is_full_clean_preset(&request.preset);

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
            details.push(MetadataStripExecuteDetail {
                source_path: path_str.clone(),
                stripped_tags: 0,
                stripped_iptc: false,
                stripped_xmp: false,
                status: ExecuteStatus::Skipped,
                reason: Some("キャンセルされました".to_string()),
            });
            report_progress(OperationProgressEvent {
                operation: "metadataStrip".to_string(),
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

        match strip_metadata_from_jpeg(file, &cats, is_full_clean) {
            Ok((stripped_tags, stripped_iptc, stripped_xmp)) => {
                succeeded += 1;
                details.push(MetadataStripExecuteDetail {
                    source_path: path_str.clone(),
                    stripped_tags,
                    stripped_iptc,
                    stripped_xmp,
                    status: ExecuteStatus::Succeeded,
                    reason: None,
                });
            }
            Err(e) => {
                // "削除するメタデータが見つかりませんでした" is a skip, not a failure
                if e.contains("削除するメタデータが見つかりませんでした") {
                    skipped += 1;
                    details.push(MetadataStripExecuteDetail {
                        source_path: path_str.clone(),
                        stripped_tags: 0,
                        stripped_iptc: false,
                        stripped_xmp: false,
                        status: ExecuteStatus::Skipped,
                        reason: Some(e),
                    });
                } else {
                    failed += 1;
                    details.push(MetadataStripExecuteDetail {
                        source_path: path_str.clone(),
                        stripped_tags: 0,
                        stripped_iptc: false,
                        stripped_xmp: false,
                        status: ExecuteStatus::Failed,
                        reason: Some(e),
                    });
                }
            }
        }

        processed += 1;
        report_progress(OperationProgressEvent {
            operation: "metadataStrip".to_string(),
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
        operation: "metadataStrip".to_string(),
        processed,
        total,
        succeeded,
        failed,
        skipped,
        current_path: None,
        done: true,
        canceled,
    });

    Ok(MetadataStripExecuteResponse {
        processed: succeeded + failed + skipped,
        succeeded,
        failed,
        skipped,
        details,
    })
}
