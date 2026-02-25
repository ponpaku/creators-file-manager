use crate::error::AppError;
use crate::file_collect::collect_rename_targets;
use crate::fs_atomic::atomic_move_replace;
use crate::model::{
    CollisionPolicy, ExecuteStatus, OperationProgressEvent, PreviewStatus, RenameExecuteDetail,
    RenameExecuteResponse, RenamePreviewItem, RenamePreviewRequest, RenamePreviewResponse,
    RenameSource, RenameTemplateTag,
};
use crate::path_norm::relative_or_portable_absolute;
use chrono::{DateTime, Local, NaiveDateTime, TimeZone};
use exif::{In, Reader, Tag, Value};
use once_cell::sync::Lazy;
use rayon::prelude::*;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

#[derive(Debug, Clone)]
struct PlannedRename {
    source: PathBuf,
    destination: Option<PathBuf>,
    status: PreviewStatus,
    reason: Option<String>,
}

static ISO_DATE_TIME_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)\d{4}[-/]\d{2}[-/]\d{2}[T\s]\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})?",
    )
    .expect("failed to compile datetime regex")
});
#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

fn ffprobe_command() -> Command {
    let mut cmd = Command::new("ffprobe");
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

static FFPROBE_AVAILABLE: Lazy<bool> = Lazy::new(|| {
    ffprobe_command()
        .arg("-version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
});

pub fn is_ffprobe_available() -> bool {
    *FFPROBE_AVAILABLE
}

pub fn template_tags() -> Vec<RenameTemplateTag> {
    vec![
        RenameTemplateTag {
            token: "{capture_date:YYYYMMDD}".to_string(),
            label: "撮影日付".to_string(),
            description: "撮影日時の日付部分を挿入".to_string(),
        },
        RenameTemplateTag {
            token: "{capture_time:HHmmss}".to_string(),
            label: "撮影時刻".to_string(),
            description: "撮影日時の時刻部分を挿入".to_string(),
        },
        RenameTemplateTag {
            token: "{exec_date:YYYYMMDD}".to_string(),
            label: "実行日付".to_string(),
            description: "実行ボタンクリック時点の日付を挿入".to_string(),
        },
        RenameTemplateTag {
            token: "{exec_time:HHmmss}".to_string(),
            label: "実行時刻".to_string(),
            description: "実行ボタンクリック時点の時刻を挿入".to_string(),
        },
        RenameTemplateTag {
            token: "{seq:3}".to_string(),
            label: "通し番号".to_string(),
            description: "ファイル順にゼロ埋め通し番号を挿入".to_string(),
        },
        RenameTemplateTag {
            token: "{original}".to_string(),
            label: "元ファイル名".to_string(),
            description: "拡張子を除く元ファイル名を挿入".to_string(),
        },
        RenameTemplateTag {
            token: "{ext}".to_string(),
            label: "拡張子".to_string(),
            description: "拡張子を挿入".to_string(),
        },
    ]
}

pub fn preview<FProgress>(
    request: &RenamePreviewRequest,
    mut report_progress: FProgress,
) -> Result<RenamePreviewResponse, AppError>
where
    FProgress: FnMut(OperationProgressEvent),
{
    let preview_timestamp = Local::now();
    let ffprobe_cache = prefetch_ffprobe_datetimes(request, &mut report_progress)?;
    let plan = build_plan(request, Some(&preview_timestamp), &ffprobe_cache)?;
    let mut ready = 0usize;
    let mut skipped = 0usize;

    let items = plan
        .iter()
        .map(|item| {
            match item.status {
                PreviewStatus::Ready => ready += 1,
                PreviewStatus::Skipped => skipped += 1,
            }
            RenamePreviewItem {
                source_path: item.source.to_string_lossy().to_string(),
                destination_path: item
                    .destination
                    .as_ref()
                    .map(|path| path.to_string_lossy().to_string()),
                status: item.status.clone(),
                reason: item.reason.clone(),
            }
        })
        .collect();

    Ok(RenamePreviewResponse {
        total: ready + skipped,
        ready,
        skipped,
        items,
    })
}

pub fn execute<FCancel, FProgress>(
    request: &RenamePreviewRequest,
    is_cancelled: FCancel,
    mut report_progress: FProgress,
) -> Result<RenameExecuteResponse, AppError>
where
    FCancel: Fn() -> bool,
    FProgress: FnMut(OperationProgressEvent),
{
    let execution_timestamp = Local::now();
    let ffprobe_cache = prefetch_ffprobe_datetimes(request, &mut report_progress)?;
    let plan = build_plan(request, Some(&execution_timestamp), &ffprobe_cache)?;
    let total = plan.len();
    let mut details = Vec::with_capacity(total);
    let mut succeeded = 0usize;
    let mut failed = 0usize;
    let mut skipped = 0usize;
    let mut processed = 0usize;
    let mut canceled = false;

    // Check if any destination overlaps with a source path. Rename is a
    // move operation, so parallel execution can destroy a source file
    // before another worker reads it (e.g. 2.jpg→1.jpg and 3.jpg→2.jpg).
    // Fall back to sequential execution when this overlap is detected.
    let needs_sequential = {
        let source_keys: HashSet<String> = plan
            .iter()
            .map(|item| destination_key(&item.source))
            .collect();
        plan.iter().any(|item| {
            item.destination
                .as_ref()
                .map_or(false, |dest| source_keys.contains(&destination_key(dest)))
        })
    };

    if needs_sequential {
        for item in &plan {
            if !canceled && is_cancelled() {
                canceled = true;
            }
            let detail = execute_one_rename(item, canceled);
            processed += 1;
            match detail.status {
                ExecuteStatus::Succeeded => succeeded += 1,
                ExecuteStatus::Failed => failed += 1,
                ExecuteStatus::Skipped => skipped += 1,
            }
            let current_path = Some(detail.source_path.clone());
            details.push(detail);
            report_progress(OperationProgressEvent {
                operation: "rename".to_string(),
                processed,
                total,
                succeeded,
                failed,
                skipped,
                current_path,
                done: false,
                canceled,
            });
        }
    } else {
        let cancel_requested = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel_requested);
        let (tx, rx) = mpsc::channel::<RenameExecuteDetail>();
        let worker_plan = plan.clone();

        let worker = std::thread::spawn(move || {
            worker_plan
                .into_par_iter()
                .for_each_with(tx, |sender, item| {
                    let detail =
                        execute_one_rename(&item, worker_cancel.load(Ordering::SeqCst));
                    let _ = sender.send(detail);
                });
        });

        while processed < total {
            if !canceled && is_cancelled() {
                canceled = true;
                cancel_requested.store(true, Ordering::SeqCst);
            }

            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(detail) => {
                    processed += 1;
                    match detail.status {
                        ExecuteStatus::Succeeded => succeeded += 1,
                        ExecuteStatus::Failed => failed += 1,
                        ExecuteStatus::Skipped => skipped += 1,
                    }
                    let current_path = Some(detail.source_path.clone());
                    details.push(detail);
                    report_progress(OperationProgressEvent {
                        operation: "rename".to_string(),
                        processed,
                        total,
                        succeeded,
                        failed,
                        skipped,
                        current_path,
                        done: false,
                        canceled,
                    });
                }
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }

        let _ = worker.join();
    }

    report_progress(OperationProgressEvent {
        operation: "rename".to_string(),
        processed,
        total,
        succeeded,
        failed,
        skipped,
        current_path: None,
        done: true,
        canceled,
    });

    Ok(RenameExecuteResponse {
        processed: succeeded + failed + skipped,
        succeeded,
        failed,
        skipped,
        details,
    })
}

fn execute_one_rename(item: &PlannedRename, canceled: bool) -> RenameExecuteDetail {
    if canceled || matches!(item.status, PreviewStatus::Skipped) {
        return RenameExecuteDetail {
            source_path: item.source.to_string_lossy().to_string(),
            destination_path: item
                .destination
                .as_ref()
                .map(|path| path.to_string_lossy().to_string()),
            status: ExecuteStatus::Skipped,
            reason: if canceled {
                Some("キャンセルされました".to_string())
            } else {
                item.reason.clone()
            },
        };
    }

    let Some(destination) = item.destination.clone() else {
        return RenameExecuteDetail {
            source_path: item.source.to_string_lossy().to_string(),
            destination_path: None,
            status: ExecuteStatus::Skipped,
            reason: Some("出力先が未定です".to_string()),
        };
    };

    if destination == item.source {
        return RenameExecuteDetail {
            source_path: item.source.to_string_lossy().to_string(),
            destination_path: Some(destination.to_string_lossy().to_string()),
            status: ExecuteStatus::Skipped,
            reason: Some("変更なし".to_string()),
        };
    }

    if let Some(parent) = destination.parent() {
        if let Err(error) = fs::create_dir_all(parent) {
            return RenameExecuteDetail {
                source_path: item.source.to_string_lossy().to_string(),
                destination_path: Some(destination.to_string_lossy().to_string()),
                status: ExecuteStatus::Failed,
                reason: Some(format!("出力先フォルダの作成に失敗しました: {}", error)),
            };
        }
    }

    match atomic_move_replace(&item.source, &destination) {
        Ok(note) => RenameExecuteDetail {
            source_path: item.source.to_string_lossy().to_string(),
            destination_path: Some(destination.to_string_lossy().to_string()),
            status: ExecuteStatus::Succeeded,
            reason: note,
        },
        Err(error) => RenameExecuteDetail {
            source_path: item.source.to_string_lossy().to_string(),
            destination_path: Some(destination.to_string_lossy().to_string()),
            status: ExecuteStatus::Failed,
            reason: Some(error),
        },
    }
}

/// Returns true if this file needs ffprobe for datetime extraction.
fn needs_ffprobe(path: &Path) -> bool {
    let ext = match path.extension().and_then(|e| e.to_str()) {
        Some(e) => e.to_ascii_lowercase(),
        None => return false,
    };
    // Images never need ffprobe
    if IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        return false;
    }
    // ISO BMFF / MXF have native parsers but may fall back to ffprobe
    // Other video extensions always need ffprobe
    VIDEO_EXTENSIONS.contains(&ext.as_str())
}

fn prefetch_ffprobe_datetimes<FProgress>(
    request: &RenamePreviewRequest,
    report_progress: &mut FProgress,
) -> Result<HashMap<PathBuf, Option<DateTime<Local>>>, AppError>
where
    FProgress: FnMut(OperationProgressEvent),
{
    if !request.use_ffprobe.unwrap_or(false) || !*FFPROBE_AVAILABLE {
        return Ok(HashMap::new());
    }
    if !matches!(request.source, RenameSource::CaptureThenModified) {
        return Ok(HashMap::new());
    }

    let collect = collect_rename_targets(&request.input_paths, request.include_subfolders)
        .map_err(AppError::InvalidRequest)?;

    let targets: Vec<PathBuf> = collect.files.iter().filter(|f| needs_ffprobe(f)).cloned().collect();
    if targets.is_empty() {
        return Ok(HashMap::new());
    }

    let total = targets.len();
    let processed = Arc::new(AtomicUsize::new(0));

    // Report start of ffprobe phase
    report_progress(OperationProgressEvent {
        operation: "rename".to_string(),
        processed: 0,
        total,
        succeeded: 0,
        failed: 0,
        skipped: 0,
        current_path: Some("メタデータ取得中...".to_string()),
        done: false,
        canceled: false,
    });

    let worker_processed = Arc::clone(&processed);
    let (tx, rx) = mpsc::channel::<(PathBuf, Option<DateTime<Local>>)>();

    let worker = std::thread::spawn(move || {
        targets.into_par_iter().for_each_with(
            (tx, worker_processed),
            |(sender, counter), path| {
                let dt = read_ffprobe_datetime(&path);
                counter.fetch_add(1, Ordering::SeqCst);
                let _ = sender.send((path, dt));
            },
        );
    });

    let mut cache = HashMap::with_capacity(total);
    let mut count = 0usize;
    while count < total {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok((path, dt)) => {
                cache.insert(path, dt);
                count += 1;
                report_progress(OperationProgressEvent {
                    operation: "rename".to_string(),
                    processed: count,
                    total,
                    succeeded: count,
                    failed: 0,
                    skipped: 0,
                    current_path: Some("メタデータ取得中...".to_string()),
                    done: false,
                    canceled: false,
                });
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    let _ = worker.join();

    // Clear progress so it doesn't linger
    report_progress(OperationProgressEvent {
        operation: "rename".to_string(),
        processed: 0,
        total: 0,
        succeeded: 0,
        failed: 0,
        skipped: 0,
        current_path: None,
        done: true,
        canceled: false,
    });

    Ok(cache)
}

fn build_plan(
    request: &RenamePreviewRequest,
    execution_timestamp: Option<&DateTime<Local>>,
    ffprobe_cache: &HashMap<PathBuf, Option<DateTime<Local>>>,
) -> Result<Vec<PlannedRename>, AppError> {
    if request.template.trim().is_empty() {
        return Err(AppError::InvalidRequest(
            "テンプレートを入力してください。".to_string(),
        ));
    }

    let collect = collect_rename_targets(&request.input_paths, request.include_subfolders)
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

    if request.output_dir.as_deref().is_some()
        && !collect.single_input_root
        && collect.files.len() > 1
    {
        return Err(AppError::InvalidRequest(
            "異なるフォルダのファイルに出力先フォルダを指定する場合、共通の親フォルダが必要です。".to_string(),
        ));
    }

    let output_dir = request.output_dir.as_ref().map(PathBuf::from);
    let template_uses_ext = request.template.contains("{ext}");
    let requires_capture_datetime = requires_capture_datetime_placeholder(&request.template);
    let conflict_policy = request
        .conflict_policy
        .clone()
        .unwrap_or(CollisionPolicy::Sequence);

    let mut used_destination_keys: HashSet<String> = HashSet::new();
    let mut planned = Vec::with_capacity(collect.files.len());

    for (index, file) in collect.files.iter().enumerate() {
        let original_stem = file
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("file")
            .to_string();
        let original_ext = file
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        let timestamp_result =
            resolve_timestamp(file, &request.source, ffprobe_cache);
        if requires_capture_datetime && timestamp_result.is_none() {
            planned.push(PlannedRename {
                source: file.clone(),
                destination: None,
                status: PreviewStatus::Skipped,
                reason: Some("タイムスタンプを取得できません".to_string()),
            });
            continue;
        }

        let (timestamp, timestamp_source) = match &timestamp_result {
            Some((dt, src)) => (Some(dt), Some(*src)),
            None => (None, None),
        };

        let rendered = render_template(
            &request.template,
            TemplateContext {
                capture_timestamp: timestamp,
                execution_timestamp,
                sequence: index + 1,
                original: &original_stem,
                ext: &original_ext,
            },
        );

        let rendered_name = match rendered {
            Ok(name) => name,
            Err(error) => {
                planned.push(PlannedRename {
                    source: file.clone(),
                    destination: None,
                    status: PreviewStatus::Skipped,
                    reason: Some(error),
                });
                continue;
            }
        };

        let mut safe_name = sanitize_file_name(&rendered_name);
        if safe_name.is_empty() {
            safe_name = original_stem.clone();
        }
        if !template_uses_ext && !original_ext.is_empty() {
            safe_name.push('.');
            safe_name.push_str(&original_ext);
        }

        let base_destination = if let Some(out_dir) = output_dir.as_ref() {
            let relative = relative_or_portable_absolute(file, collect.input_root.as_deref());
            let relative_parent = relative.parent().map_or_else(PathBuf::new, PathBuf::from);
            out_dir.join(relative_parent).join(safe_name)
        } else {
            file.parent().map_or_else(
                || PathBuf::from(&safe_name),
                |parent| parent.join(&safe_name),
            )
        };

        let (status, collision_reason, destination) = resolve_destination_for_policy(
            &base_destination,
            file,
            &mut used_destination_keys,
            &conflict_policy,
        );

        let reason = if requires_capture_datetime {
            match (timestamp_source, collision_reason) {
                (Some(src), Some(col)) => Some(format!("{} / {}", src, col)),
                (Some(src), None) => Some(src.to_string()),
                (None, col) => col,
            }
        } else {
            collision_reason
        };

        planned.push(PlannedRename {
            source: file.clone(),
            destination: Some(destination),
            status,
            reason,
        });
    }

    // When overwrite policy is used and multiple sources map to the same
    // destination, keep only the last writer (by sorted order) as Ready.
    // Earlier duplicates become Skipped to avoid nondeterministic races
    // during parallel execution.
    if matches!(conflict_policy, CollisionPolicy::Overwrite) {
        let mut last_ready: HashMap<String, usize> = HashMap::new();
        for (i, item) in planned.iter().enumerate() {
            if matches!(item.status, PreviewStatus::Ready) {
                if let Some(dest) = &item.destination {
                    let key = destination_key(dest);
                    last_ready.insert(key, i);
                }
            }
        }
        for (i, item) in planned.iter_mut().enumerate() {
            if !matches!(item.status, PreviewStatus::Ready) {
                continue;
            }
            if let Some(dest) = &item.destination {
                let key = destination_key(dest);
                if let Some(&last) = last_ready.get(&key) {
                    if i < last {
                        item.status = PreviewStatus::Skipped;
                        item.reason = Some(
                            "同一出力先の後続ファイルに置き換えられました".to_string(),
                        );
                    }
                }
            }
        }
    }

    Ok(planned)
}

fn resolve_timestamp(
    path: &Path,
    source: &RenameSource,
    ffprobe_cache: &HashMap<PathBuf, Option<DateTime<Local>>>,
) -> Option<(DateTime<Local>, &'static str)> {
    match source {
        RenameSource::CurrentTime => Some((Local::now(), "現在時刻")),
        RenameSource::ModifiedOnly => {
            read_modified_datetime(path).map(|dt| (dt, "ファイル更新日時"))
        }
        RenameSource::CaptureThenModified => read_capture_datetime(path, ffprobe_cache)
            .or_else(|| read_modified_datetime(path).map(|dt| (dt, "ファイル更新日時"))),
    }
}

fn read_modified_datetime(path: &Path) -> Option<DateTime<Local>> {
    let metadata = fs::metadata(path).ok()?;
    let modified = metadata.modified().ok()?;
    Some(DateTime::<Local>::from(modified))
}

const IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "webp", "gif", "tif", "tiff", "bmp",
    "heic", "heif", "dng", "cr2", "cr3", "nef", "arw", "raf",
];

const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "mov", "m4v", "avi", "mkv", "wmv", "mts", "m2ts",
    "mpg", "mpeg", "webm", "mxf",
];

fn read_capture_datetime(
    path: &Path,
    ffprobe_cache: &HashMap<PathBuf, Option<DateTime<Local>>>,
) -> Option<(DateTime<Local>, &'static str)> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();

    // Images: try EXIF only, never ffprobe
    if IMAGE_EXTENSIONS.contains(&extension.as_str()) {
        return read_image_capture_datetime(path).map(|dt| (dt, "EXIF"));
    }

    // Videos: try native parsing first, then ffprobe cache as fallback
    if matches!(extension.as_str(), "mp4" | "mov" | "m4v") {
        return read_iso_bmff_creation_datetime(path)
            .map(|dt| (dt, "メタデータ"))
            .or_else(|| {
                ffprobe_cache
                    .get(path)
                    .copied()
                    .flatten()
                    .map(|dt| (dt, "ffprobe"))
            });
    }
    if extension == "mxf" {
        return read_mxf_sidecar_datetime(path)
            .map(|dt| (dt, "XMLサイドカー"))
            .or_else(|| {
                ffprobe_cache
                    .get(path)
                    .copied()
                    .flatten()
                    .map(|dt| (dt, "ffprobe"))
            });
    }
    if VIDEO_EXTENSIONS.contains(&extension.as_str()) {
        return ffprobe_cache
            .get(path)
            .copied()
            .flatten()
            .map(|dt| (dt, "ffprobe"));
    }
    None
}

fn read_image_capture_datetime(path: &Path) -> Option<DateTime<Local>> {
    let file = fs::File::open(path).ok()?;
    let mut reader = BufReader::new(file);
    let exif = Reader::new().read_from_container(&mut reader).ok()?;
    let field = exif
        .get_field(Tag::DateTimeOriginal, In::PRIMARY)
        .or_else(|| exif.get_field(Tag::DateTime, In::PRIMARY))?;
    // Extract the raw ASCII bytes directly instead of display_value(),
    // which wraps the string in double quotes causing parse failure.
    let date_value = match &field.value {
        Value::Ascii(ref vec) if !vec.is_empty() => {
            String::from_utf8(vec[0].clone()).ok()
        }
        _ => None,
    }
    .unwrap_or_else(|| {
        field.display_value().with_unit(&exif).to_string()
    });
    parse_exif_datetime(&date_value)
}

#[derive(Debug, Clone, Copy)]
struct AtomRange {
    data_start: u64,
    data_end: u64,
}

fn read_iso_bmff_creation_datetime(path: &Path) -> Option<DateTime<Local>> {
    let mut file = fs::File::open(path).ok()?;
    let file_len = file.metadata().ok()?.len();
    let moov = find_atom(&mut file, 0, file_len, *b"moov")?;
    let mvhd = find_atom(&mut file, moov.data_start, moov.data_end, *b"mvhd")?;
    parse_mvhd_creation_time(&mut file, mvhd)
}

fn find_atom(file: &mut fs::File, start: u64, end: u64, atom_type: [u8; 4]) -> Option<AtomRange> {
    let mut offset = start;
    while offset + 8 <= end {
        file.seek(SeekFrom::Start(offset)).ok()?;
        let mut header = [0u8; 8];
        file.read_exact(&mut header).ok()?;
        let mut atom_size = u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as u64;
        let atom_kind = [header[4], header[5], header[6], header[7]];
        let mut header_size = 8u64;

        if atom_size == 1 {
            let mut ext = [0u8; 8];
            file.read_exact(&mut ext).ok()?;
            atom_size = u64::from_be_bytes(ext);
            header_size = 16;
        } else if atom_size == 0 {
            atom_size = end.saturating_sub(offset);
        }
        if atom_size < header_size {
            return None;
        }
        let atom_end = offset.saturating_add(atom_size).min(end);
        if atom_end <= offset {
            return None;
        }

        if atom_kind == atom_type {
            return Some(AtomRange {
                data_start: offset + header_size,
                data_end: atom_end,
            });
        }
        offset = atom_end;
    }
    None
}

fn parse_mvhd_creation_time(file: &mut fs::File, mvhd: AtomRange) -> Option<DateTime<Local>> {
    file.seek(SeekFrom::Start(mvhd.data_start)).ok()?;
    let mut ver_flags = [0u8; 4];
    file.read_exact(&mut ver_flags).ok()?;
    let version = ver_flags[0];
    let qt_seconds = if version == 1 {
        read_u64_be(file)?
    } else {
        read_u32_be(file)? as u64
    };
    qt_epoch_seconds_to_local(qt_seconds)
}

fn read_u32_be(file: &mut fs::File) -> Option<u32> {
    let mut buf = [0u8; 4];
    file.read_exact(&mut buf).ok()?;
    Some(u32::from_be_bytes(buf))
}

fn read_u64_be(file: &mut fs::File) -> Option<u64> {
    let mut buf = [0u8; 8];
    file.read_exact(&mut buf).ok()?;
    Some(u64::from_be_bytes(buf))
}

fn qt_epoch_seconds_to_local(qt_seconds: u64) -> Option<DateTime<Local>> {
    const QT_TO_UNIX_OFFSET: i64 = 2_082_844_800;
    let unix = (qt_seconds as i64).checked_sub(QT_TO_UNIX_OFFSET)?;
    let utc = chrono::DateTime::<chrono::Utc>::from_timestamp(unix, 0)?;
    Some(utc.with_timezone(&Local))
}

fn read_mxf_sidecar_datetime(path: &Path) -> Option<DateTime<Local>> {
    let stem = path.file_stem()?.to_str()?.to_ascii_lowercase();
    let mut candidates = Vec::new();

    candidates.push(path.with_extension("xml"));
    if let Some(parent) = path.parent() {
        let entries = fs::read_dir(parent).ok()?;
        for entry in entries.filter_map(Result::ok) {
            let p = entry.path();
            let ext = p.extension().and_then(|x| x.to_str()).unwrap_or("");
            if !ext.eq_ignore_ascii_case("xml") {
                continue;
            }
            let file_stem = p
                .file_stem()
                .and_then(|x| x.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if file_stem == stem || file_stem.starts_with(&stem) {
                candidates.push(p);
            }
        }
    }

    for candidate in candidates {
        if !candidate.exists() || !candidate.is_file() {
            continue;
        }
        if let Ok(text) = fs::read_to_string(candidate) {
            if let Some(dt) = parse_datetime_from_xml_text(&text) {
                return Some(dt);
            }
        }
    }
    None
}

fn parse_datetime_from_xml_text(text: &str) -> Option<DateTime<Local>> {
    let key_tokens = ["creation", "record", "shoot", "start", "date", "time"];
    for line in text.lines() {
        let lower = line.to_ascii_lowercase();
        if !key_tokens.iter().any(|key| lower.contains(key)) {
            continue;
        }
        for capture in ISO_DATE_TIME_RE.find_iter(line) {
            if let Some(dt) = parse_loose_datetime(capture.as_str()) {
                return Some(dt);
            }
        }
    }
    for capture in ISO_DATE_TIME_RE.find_iter(text) {
        if let Some(dt) = parse_loose_datetime(capture.as_str()) {
            return Some(dt);
        }
    }
    None
}

fn parse_loose_datetime(value: &str) -> Option<DateTime<Local>> {
    let normalized = value.replace('/', "-");
    let with_timezone = if normalized.ends_with('Z')
        || normalized.contains('+')
        || (normalized.len() > 10 && normalized[10..].contains('-'))
    {
        normalize_rfc3339_timezone(&normalized)
    } else {
        normalized.clone()
    };

    if let Ok(dt) = DateTime::parse_from_rfc3339(&with_timezone) {
        return Some(dt.with_timezone(&Local));
    }

    let formats = [
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y:%m:%d %H:%M:%S",
    ];
    for format in formats {
        if let Ok(naive) = NaiveDateTime::parse_from_str(&normalized, format) {
            if let Some(local) = Local
                .from_local_datetime(&naive)
                .single()
                .or_else(|| Local.from_local_datetime(&naive).earliest())
            {
                return Some(local);
            }
        }
    }
    None
}

fn normalize_rfc3339_timezone(value: &str) -> String {
    if value.ends_with('Z') {
        return value.to_string();
    }
    if value.len() >= 5 {
        let tz = &value[value.len() - 5..];
        if (tz.starts_with('+') || tz.starts_with('-'))
            && tz.chars().skip(1).all(|c| c.is_ascii_digit())
        {
            let (head, tail) = value.split_at(value.len() - 5);
            return format!("{}{}:{}", head, &tail[..3], &tail[3..]);
        }
    }
    value.to_string()
}

fn read_ffprobe_datetime(path: &Path) -> Option<DateTime<Local>> {
    if !*FFPROBE_AVAILABLE {
        return None;
    }
    let output = ffprobe_command()
        .arg("-v")
        .arg("error")
        .arg("-show_entries")
        .arg("format_tags=creation_time:stream_tags=creation_time")
        .arg("-of")
        .arg("default=nokey=1:noprint_wrappers=1")
        .arg(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if let Some(dt) = parse_loose_datetime(line.trim()) {
            return Some(dt);
        }
        for capture in ISO_DATE_TIME_RE.find_iter(line) {
            if let Some(dt) = parse_loose_datetime(capture.as_str()) {
                return Some(dt);
            }
        }
    }
    None
}

fn parse_exif_datetime(value: &str) -> Option<DateTime<Local>> {
    let trimmed = value.trim().trim_matches('"');
    let naive = NaiveDateTime::parse_from_str(trimmed, "%Y:%m:%d %H:%M:%S").ok()?;
    Local
        .from_local_datetime(&naive)
        .single()
        .or_else(|| Local.from_local_datetime(&naive).earliest())
}

fn requires_capture_datetime_placeholder(template: &str) -> bool {
    template.contains("{capture_date") || template.contains("{capture_time")
}

struct TemplateContext<'a> {
    capture_timestamp: Option<&'a DateTime<Local>>,
    execution_timestamp: Option<&'a DateTime<Local>>,
    sequence: usize,
    original: &'a str,
    ext: &'a str,
}

fn render_template(template: &str, context: TemplateContext<'_>) -> Result<String, String> {
    let chars: Vec<char> = template.chars().collect();
    let mut output = String::new();
    let mut index = 0usize;

    while index < chars.len() {
        if chars[index] != '{' {
            output.push(chars[index]);
            index += 1;
            continue;
        }

        let mut end = index + 1;
        while end < chars.len() && chars[end] != '}' {
            end += 1;
        }
        if end >= chars.len() {
            return Err("テンプレートに閉じられていない `{` があります".to_string());
        }

        let token: String = chars[index + 1..end].iter().collect();
        let replacement = resolve_token(&token, &context)?;
        output.push_str(&replacement);
        index = end + 1;
    }

    Ok(output)
}

fn resolve_token(token: &str, context: &TemplateContext<'_>) -> Result<String, String> {
    let (key, arg) = token
        .split_once(':')
        .map_or((token, None), |(k, v)| (k, Some(v)));

    match key {
        "capture_date" => {
            let timestamp = context
                .capture_timestamp
                .ok_or_else(|| "{capture_date} には撮影日時が必要です".to_string())?;
            let format = convert_datetime_format(arg.unwrap_or("YYYYMMDD"));
            Ok(timestamp.format(&format).to_string())
        }
        "capture_time" => {
            let timestamp = context
                .capture_timestamp
                .ok_or_else(|| "{capture_time} には撮影日時が必要です".to_string())?;
            let format = convert_datetime_format(arg.unwrap_or("HHmmss"));
            Ok(timestamp.format(&format).to_string())
        }
        "exec_date" => {
            let timestamp = context
                .execution_timestamp
                .ok_or_else(|| "{exec_date} には実行日時が必要です".to_string())?;
            let format = convert_datetime_format(arg.unwrap_or("YYYYMMDD"));
            Ok(timestamp.format(&format).to_string())
        }
        "exec_time" => {
            let timestamp = context
                .execution_timestamp
                .ok_or_else(|| "{exec_time} には実行日時が必要です".to_string())?;
            let format = convert_datetime_format(arg.unwrap_or("HHmmss"));
            Ok(timestamp.format(&format).to_string())
        }
        "seq" => {
            let digits = arg.unwrap_or("1");
            let digits: usize = digits
                .parse()
                .map_err(|_| "seq の桁数は正の整数で指定してください".to_string())?;
            if digits == 0 {
                return Err("seq の桁数は1以上にしてください".to_string());
            }
            Ok(format!("{:0width$}", context.sequence, width = digits))
        }
        "original" => Ok(context.original.to_string()),
        "ext" => Ok(context.ext.to_string()),
        _ => Err(format!("未対応のプレースホルダー: {{{}}}", token)),
    }
}

fn convert_datetime_format(value: &str) -> String {
    value
        .replace("YYYY", "%Y")
        .replace("MM", "%m")
        .replace("DD", "%d")
        .replace("HH", "%H")
        .replace("mm", "%M")
        .replace("ss", "%S")
}

fn sanitize_file_name(value: &str) -> String {
    let invalid_chars = ['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
    let sanitized: String = value
        .chars()
        .map(|ch| if invalid_chars.contains(&ch) { '_' } else { ch })
        .collect();
    sanitized.trim().trim_matches('.').to_string()
}

fn uniquify_destination(base: &Path, source: &Path, used_keys: &mut HashSet<String>) -> PathBuf {
    let mut candidate = base.to_path_buf();
    let mut suffix = 1usize;

    loop {
        let key = destination_key(&candidate);
        let already_planned = used_keys.contains(&key);
        let already_exists = candidate.exists() && candidate != source;
        if !already_planned && !already_exists {
            used_keys.insert(key);
            return candidate;
        }

        let stem = base
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("file");
        let extension = base.extension().and_then(|ext| ext.to_str()).unwrap_or("");
        let file_name = if extension.is_empty() {
            format!("{}_{}", stem, suffix)
        } else {
            format!("{}_{}.{}", stem, suffix, extension)
        };
        candidate = base.parent().map_or_else(
            || PathBuf::from(&file_name),
            |parent| parent.join(&file_name),
        );
        suffix += 1;
    }
}

fn destination_key(path: &Path) -> String {
    path.to_string_lossy().to_ascii_lowercase()
}

fn resolve_destination_for_policy(
    base: &Path,
    source: &Path,
    used_keys: &mut HashSet<String>,
    policy: &CollisionPolicy,
) -> (PreviewStatus, Option<String>, PathBuf) {
    let key = destination_key(base);
    let collision = used_keys.contains(&key) || (base.exists() && base != source);
    match policy {
        CollisionPolicy::Overwrite => {
            used_keys.insert(key);
            (
                PreviewStatus::Ready,
                if collision {
                    Some("競合ポリシーにより上書きされます".to_string())
                } else {
                    None
                },
                base.to_path_buf(),
            )
        }
        CollisionPolicy::Skip => {
            if collision {
                (
                    PreviewStatus::Skipped,
                    Some("出力先の競合によりスキップされました".to_string()),
                    base.to_path_buf(),
                )
            } else {
                used_keys.insert(key);
                (PreviewStatus::Ready, None, base.to_path_buf())
            }
        }
        CollisionPolicy::Sequence => {
            let unique = uniquify_destination(base, source, used_keys);
            (
                PreviewStatus::Ready,
                if unique != base {
                    Some("競合のため連番サフィックスを付与しました".to_string())
                } else {
                    None
                },
                unique,
            )
        }
    }
}
