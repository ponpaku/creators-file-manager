use crate::error::AppError;
use crate::file_collect::collect_targets_with_extensions;
use crate::model::{
    CollisionPolicy, OperationProgressEvent, PreviewStatus, ResizeCollectInfoResponse,
    ResizeExecuteResponse, ResizePreviewItem, ResizePreviewRequest, ResizePreviewResponse,
};
use crate::path_norm::relative_or_portable_absolute;
use crate::worker_bridge::{self, BatchProgress, ResizeBatchItemMsg};
use chrono::Local;
use image::ImageReader;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use tauri::AppHandle;

pub const RESIZE_ALLOWED_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp"];

#[derive(Debug, Clone)]
struct PlannedResize {
    source: PathBuf,
    destination: PathBuf,
    source_size: u64,
    original_width: u32,
    original_height: u32,
    new_width: u32,
    new_height: u32,
    status: PreviewStatus,
    reason: Option<String>,
}

pub fn collect_info(
    input_paths: &[String],
    include_subfolders: bool,
) -> Result<ResizeCollectInfoResponse, AppError> {
    let collect =
        collect_targets_with_extensions(input_paths, include_subfolders, RESIZE_ALLOWED_EXTENSIONS)
            .map_err(AppError::InvalidRequest)?;
    let total_size: u64 = collect
        .files
        .iter()
        .map(|path| fs::metadata(path).map(|m| m.len()).unwrap_or(0))
        .sum();
    Ok(ResizeCollectInfoResponse {
        file_count: collect.files.len(),
        total_size,
    })
}

pub fn preview(request: &ResizePreviewRequest) -> Result<ResizePreviewResponse, AppError> {
    let plan = build_plan(request)?;
    Ok(response_from_plan(&plan.items, &plan.output_dir))
}

pub fn execute<FCancel, FProgress>(
    app: &AppHandle,
    request: &ResizePreviewRequest,
    is_cancelled: FCancel,
    mut report_progress: FProgress,
) -> Result<ResizeExecuteResponse, AppError>
where
    FCancel: Fn() -> bool,
    FProgress: FnMut(OperationProgressEvent),
{
    let plan = build_plan(request)?;
    fs::create_dir_all(&plan.output_dir)?;

    let total = plan.items.len();

    let items: Vec<ResizeBatchItemMsg> = plan
        .items
        .iter()
        .map(|item| ResizeBatchItemMsg {
            source: item.source.to_string_lossy().to_string(),
            destination: item.destination.to_string_lossy().to_string(),
            skip: matches!(item.status, PreviewStatus::Skipped),
        })
        .collect();

    let mut succeeded = 0usize;
    let mut failed = 0usize;
    let mut skipped = 0usize;
    let mut processed = 0usize;
    let mut canceled = false;

    let result = worker_bridge::resize_batch(
        app,
        items,
        request.mode.clone(),
        request.size_px,
        request.small_image_policy.clone(),
        request.filter.clone(),
        request.sharpen,
        request.quality,
        request.preserve_exif,
        || {
            if is_cancelled() {
                true
            } else {
                false
            }
        },
        |progress: BatchProgress| {
            processed += 1;
            match progress.status.as_str() {
                "succeeded" => succeeded += 1,
                "failed" => failed += 1,
                _ => skipped += 1,
            }

            if !canceled && is_cancelled() {
                canceled = true;
            }

            report_progress(OperationProgressEvent {
                operation: "resize".to_string(),
                processed,
                total,
                succeeded,
                failed,
                skipped,
                current_path: Some(progress.source),
                done: false,
                canceled,
            });
        },
    )
    .map_err(|e| AppError::Io(e))?;

    succeeded = result.succeeded;
    failed = result.failed;
    skipped = result.skipped;

    report_progress(OperationProgressEvent {
        operation: "resize".to_string(),
        processed: succeeded + failed + skipped,
        total,
        succeeded,
        failed,
        skipped,
        current_path: None,
        done: true,
        canceled,
    });

    Ok(ResizeExecuteResponse {
        output_dir: plan.output_dir.to_string_lossy().to_string(),
        succeeded,
        failed,
        skipped,
    })
}

struct ResizePlan {
    output_dir: PathBuf,
    items: Vec<PlannedResize>,
}

fn build_plan(request: &ResizePreviewRequest) -> Result<ResizePlan, AppError> {
    let collect = collect_targets_with_extensions(
        &request.input_paths,
        request.include_subfolders,
        RESIZE_ALLOWED_EXTENSIONS,
    )
    .map_err(AppError::InvalidRequest)?;

    if collect.files.is_empty() {
        let msg = if collect.skipped_by_extension > 0 {
            format!(
                "対応していないファイル形式です（{}件のファイルがスキップされました。JPEG・PNG・WebP のみ対応）",
                collect.skipped_by_extension
            )
        } else {
            "対象のファイルが見つかりません".to_string()
        };
        return Err(AppError::InvalidRequest(msg));
    }

    if request.output_dir.as_deref().is_none()
        && !collect.single_input_root
        && collect.files.len() > 1
    {
        return Err(AppError::InvalidRequest(
            "異なるフォルダのファイルには出力先フォルダの指定が必要です".to_string(),
        ));
    }

    let output_dir = resolve_output_dir(
        collect.input_root.as_deref(),
        request.output_dir.as_deref(),
        "_resized_",
    )?;

    let mut items = Vec::with_capacity(collect.files.len());
    let mut used_destinations: HashSet<String> = HashSet::new();

    for source in &collect.files {
        let source_size = fs::metadata(source).map(|m| m.len()).unwrap_or(0);

        // Read image dimensions quickly (header only)
        let (original_width, original_height) = read_dimensions(source)?;

        let current_side = if request.mode == "short_side" {
            original_width.min(original_height)
        } else {
            original_width.max(original_height)
        };

        let is_small = current_side <= request.size_px;

        let (new_width, new_height, status, reason) = if is_small
            && request.small_image_policy == "skip"
        {
            (original_width, original_height, PreviewStatus::Skipped, Some("小さい画像のためスキップ".to_string()))
        } else if is_small && request.small_image_policy == "copy" {
            (original_width, original_height, PreviewStatus::Skipped, Some("小さい画像のためコピーのみ".to_string()))
        } else {
            let scale = request.size_px as f32 / current_side as f32;
            let nw = ((original_width as f32) * scale).round().max(1.0) as u32;
            let nh = ((original_height as f32) * scale).round().max(1.0) as u32;
            (nw, nh, PreviewStatus::Ready, None)
        };

        let relative = relative_or_portable_absolute(source, collect.input_root.as_deref());
        let base_destination = output_dir.join(relative);

        let (dest_status, dest_reason, destination) = resolve_destination_for_policy(
            &base_destination,
            &mut used_destinations,
            request.conflict_policy.clone(),
        );

        let final_status = if matches!(status, PreviewStatus::Skipped) {
            status
        } else {
            dest_status
        };
        let combined_reason = match (reason, dest_reason) {
            (Some(a), Some(b)) => Some(format!("{}; {}", a, b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };

        items.push(PlannedResize {
            source: source.clone(),
            destination,
            source_size,
            original_width,
            original_height,
            new_width,
            new_height,
            status: final_status,
            reason: combined_reason,
        });
    }

    Ok(ResizePlan { output_dir, items })
}

fn read_dimensions(path: &Path) -> Result<(u32, u32), AppError> {
    let reader = ImageReader::open(path)
        .map_err(|e| AppError::Io(format!("画像ファイルを開けません: {}", e)))?
        .with_guessed_format()
        .map_err(|e| AppError::Io(format!("フォーマット判定に失敗しました: {}", e)))?;
    let (w, h) = reader
        .into_dimensions()
        .map_err(|e| AppError::Io(format!("画像サイズの取得に失敗しました: {}", e)))?;
    Ok((w, h))
}

fn response_from_plan(items: &[PlannedResize], _output_dir: &Path) -> ResizePreviewResponse {
    let mut ready = 0usize;
    let mut skipped = 0usize;
    let result_items = items
        .iter()
        .map(|item| {
            match item.status {
                PreviewStatus::Ready => ready += 1,
                PreviewStatus::Skipped => skipped += 1,
            }
            ResizePreviewItem {
                source_path: item.source.to_string_lossy().to_string(),
                destination_path: item.destination.to_string_lossy().to_string(),
                source_size: item.source_size,
                original_width: item.original_width,
                original_height: item.original_height,
                new_width: item.new_width,
                new_height: item.new_height,
                status: item.status.clone(),
                reason: item.reason.clone(),
            }
        })
        .collect();

    ResizePreviewResponse {
        items: result_items,
        total: ready + skipped,
        ready,
        skipped,
    }
}

fn resolve_output_dir(
    input_root: Option<&Path>,
    output_dir: Option<&str>,
    suffix_tag: &str,
) -> Result<PathBuf, AppError> {
    if let Some(raw) = output_dir {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(AppError::InvalidRequest(
                "出力先フォルダを指定してください".to_string(),
            ));
        }
        return Ok(PathBuf::from(trimmed));
    }
    let input_root = input_root.ok_or_else(|| {
        AppError::InvalidRequest(
            "共通の入力ルートがないため出力フォルダを自動生成できません".to_string(),
        )
    })?;
    let parent = input_root
        .parent()
        .ok_or_else(|| AppError::InvalidRequest("親フォルダを特定できません".to_string()))?;
    let dirname = input_root
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| AppError::InvalidRequest("入力ルートの名前が無効です".to_string()))?;
    let timestamp = Local::now().format("%Y%m%d%H%M%S");
    let candidate = parent.join(format!("{}{}{}", dirname, suffix_tag, timestamp));
    Ok(uniquify_directory(candidate))
}

fn resolve_destination_for_policy(
    base_destination: &Path,
    used_destinations: &mut HashSet<String>,
    policy: CollisionPolicy,
) -> (PreviewStatus, Option<String>, PathBuf) {
    let key = base_destination.to_string_lossy().to_ascii_lowercase();
    let collision = used_destinations.contains(&key) || base_destination.exists();

    match policy {
        CollisionPolicy::Overwrite => {
            used_destinations.insert(key);
            (
                PreviewStatus::Ready,
                if collision {
                    Some("競合ポリシーにより上書きされます".to_string())
                } else {
                    None
                },
                base_destination.to_path_buf(),
            )
        }
        CollisionPolicy::Skip => {
            if collision {
                (
                    PreviewStatus::Skipped,
                    Some("出力先の競合によりスキップされました".to_string()),
                    base_destination.to_path_buf(),
                )
            } else {
                used_destinations.insert(key);
                (PreviewStatus::Ready, None, base_destination.to_path_buf())
            }
        }
        CollisionPolicy::Sequence => {
            let destination = uniquify_destination(base_destination, used_destinations);
            (
                PreviewStatus::Ready,
                if destination != base_destination {
                    Some("競合のため連番サフィックスを付与しました".to_string())
                } else {
                    None
                },
                destination,
            )
        }
    }
}

fn uniquify_directory(base: PathBuf) -> PathBuf {
    if !base.exists() {
        return base;
    }
    let parent = base
        .parent()
        .map_or_else(|| PathBuf::from("."), PathBuf::from);
    let stem = base
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("resized")
        .to_string();
    let mut index = 1usize;
    loop {
        let candidate = parent.join(format!("{}_no{}", stem, index));
        if !candidate.exists() {
            return candidate;
        }
        index += 1;
    }
}

fn uniquify_destination(base: &Path, used_destinations: &mut HashSet<String>) -> PathBuf {
    let mut candidate = base.to_path_buf();
    let mut suffix = 1usize;
    loop {
        let key = candidate.to_string_lossy().to_ascii_lowercase();
        if !used_destinations.contains(&key) && !candidate.exists() {
            used_destinations.insert(key);
            return candidate;
        }
        let stem = base
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("file");
        let ext = base
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        let file_name = if ext.is_empty() {
            format!("{}_{}", stem, suffix)
        } else {
            format!("{}_{}.{}", stem, suffix, ext)
        };
        candidate = base.parent().map_or_else(
            || PathBuf::from(&file_name),
            |parent| parent.join(&file_name),
        );
        suffix += 1;
    }
}
