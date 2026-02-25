use crate::error::AppError;
use crate::file_collect::{collect_targets_with_extensions, JPEG_ALLOWED_EXTENSIONS};
use crate::model::{
    CollisionPolicy, CompressCollectInfoResponse, CompressEstimateResponse,
    CompressExecuteDetail, CompressExecuteResponse, CompressPreviewItem, CompressPreviewRequest,
    CompressPreviewResponse, EstimateProgressEvent, ExecuteStatus, OperationProgressEvent,
    PreviewStatus,
};
use crate::path_norm::relative_or_portable_absolute;
use crate::worker_bridge::{self, BatchProgress, CompressBatchItemMsg};
use chrono::Local;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use tauri::AppHandle;

#[derive(Debug, Clone)]
struct PlannedCompress {
    source: PathBuf,
    destination: PathBuf,
    source_size: u64,
    estimated_size: u64,
    status: PreviewStatus,
    reason: Option<String>,
}

pub fn collect_info(
    input_paths: &[String],
    include_subfolders: bool,
) -> Result<CompressCollectInfoResponse, AppError> {
    let collect = collect_targets_with_extensions(input_paths, include_subfolders, JPEG_ALLOWED_EXTENSIONS)
        .map_err(AppError::InvalidRequest)?;
    let total_size: u64 = collect
        .files
        .iter()
        .map(|path| fs::metadata(path).map(|m| m.len()).unwrap_or(0))
        .sum();
    Ok(CompressCollectInfoResponse {
        file_count: collect.files.len(),
        total_size,
    })
}

pub fn estimate(
    app: &AppHandle,
    input_paths: &[String],
    include_subfolders: bool,
    resize_percent: f32,
    quality: u8,
    is_cancelled: impl Fn() -> bool + Sync,
    on_progress: impl Fn(EstimateProgressEvent) + Sync,
) -> Result<CompressEstimateResponse, AppError> {
    let collect =
        collect_targets_with_extensions(input_paths, include_subfolders, JPEG_ALLOWED_EXTENSIONS)
            .map_err(AppError::InvalidRequest)?;
    let total_source_size: u64 = collect
        .files
        .iter()
        .map(|path| fs::metadata(path).map(|m| m.len()).unwrap_or(0))
        .sum();
    if collect.files.is_empty() || total_source_size == 0 {
        return Ok(CompressEstimateResponse {
            file_count: collect.files.len(),
            total_source_size,
            estimated_total_size: 0,
        });
    }

    let file_strings: Vec<String> = collect
        .files
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();

    let ratio = worker_bridge::sample_estimate(
        app,
        file_strings,
        resize_percent.clamp(1.0, 100.0),
        quality.clamp(1, 100),
        10,
        &is_cancelled,
        |current, total| {
            on_progress(EstimateProgressEvent { current, total });
        },
    )
    .map_err(|e| AppError::Io(e))?;

    let estimated_total_size = ((total_source_size as f64) * ratio).round() as u64;
    Ok(CompressEstimateResponse {
        file_count: collect.files.len(),
        total_source_size,
        estimated_total_size,
    })
}

pub fn preview(request: &CompressPreviewRequest, app: &AppHandle) -> Result<CompressPreviewResponse, AppError> {
    let state = build_plan(request, app)?;
    Ok(preview_response_from_state(&state))
}

pub fn execute<FCancel, FProgress>(
    app: &AppHandle,
    request: &CompressPreviewRequest,
    is_cancelled: FCancel,
    mut report_progress: FProgress,
) -> Result<CompressExecuteResponse, AppError>
where
    FCancel: Fn() -> bool,
    FProgress: FnMut(OperationProgressEvent),
{
    let state = build_plan(request, app)?;
    fs::create_dir_all(&state.output_dir)?;

    let total = state.plan.len();

    // Build worker batch items
    let items: Vec<CompressBatchItemMsg> = state
        .plan
        .iter()
        .map(|item| CompressBatchItemMsg {
            source: item.source.to_string_lossy().to_string(),
            destination: item.destination.to_string_lossy().to_string(),
            skip: matches!(item.status, PreviewStatus::Skipped),
        })
        .collect();

    let mut details = Vec::with_capacity(total);
    let mut succeeded = 0usize;
    let mut failed = 0usize;
    let mut skipped = 0usize;
    let mut processed = 0usize;
    let mut canceled = false;

    let result = worker_bridge::compress_batch(
        app,
        items,
        state.effective_resize_percent,
        state.effective_quality,
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
            let status = match progress.status.as_str() {
                "succeeded" => {
                    succeeded += 1;
                    ExecuteStatus::Succeeded
                }
                "failed" => {
                    failed += 1;
                    ExecuteStatus::Failed
                }
                _ => {
                    skipped += 1;
                    ExecuteStatus::Skipped
                }
            };

            if !canceled && is_cancelled() {
                canceled = true;
            }

            details.push(CompressExecuteDetail {
                source_path: progress.source.clone(),
                destination_path: progress.destination.clone(),
                status,
                output_size: progress.output_size,
                reason: progress.reason,
            });

            report_progress(OperationProgressEvent {
                operation: "compress".to_string(),
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

    // Use the batch result totals (more accurate for race conditions)
    succeeded = result.succeeded;
    failed = result.failed;
    skipped = result.skipped;

    report_progress(OperationProgressEvent {
        operation: "compress".to_string(),
        processed: succeeded + failed + skipped,
        total,
        succeeded,
        failed,
        skipped,
        current_path: None,
        done: true,
        canceled,
    });

    Ok(CompressExecuteResponse {
        output_dir: state.output_dir.to_string_lossy().to_string(),
        effective_resize_percent: state.effective_resize_percent,
        effective_quality: state.effective_quality,
        processed: succeeded + failed + skipped,
        succeeded,
        failed,
        skipped,
        details,
    })
}

#[derive(Debug)]
struct CompressPlanState {
    output_dir: PathBuf,
    effective_resize_percent: f32,
    effective_quality: u8,
    target_size_kb: Option<u64>,
    tolerance_percent: f32,
    plan: Vec<PlannedCompress>,
    warnings: usize,
}

fn build_plan(request: &CompressPreviewRequest, app: &AppHandle) -> Result<CompressPlanState, AppError> {
    let resize_percent = request.resize_percent.clamp(1.0, 100.0);
    let quality = request.quality.clamp(1, 100);
    let tolerance_percent = request.tolerance_percent.unwrap_or(10.0).max(0.0);

    let collect = collect_targets_with_extensions(
        &request.input_paths,
        request.include_subfolders,
        JPEG_ALLOWED_EXTENSIONS,
    )
    .map_err(AppError::InvalidRequest)?;
    if collect.files.is_empty() {
        let msg = if collect.skipped_by_extension > 0 {
            format!(
                "対応していないファイル形式です（{}件のファイルがスキップされました。JPEG のみ対応）",
                collect.skipped_by_extension
            )
        } else {
            "対象のJPEGファイルが見つかりません".to_string()
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
        "_compressed_",
    )?;

    let (effective_resize_percent, effective_quality) = match request.target_size_kb {
        Some(total_target_kb) => {
            let total_source: u64 = collect
                .files
                .iter()
                .map(|path| fs::metadata(path).map(|m| m.len()).unwrap_or(0))
                .sum();
            let file_strings: Vec<String> = collect
                .files
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect();
            let target_bytes = (total_target_kb as u64) * 1024;
            worker_bridge::suggest_params(
                app,
                file_strings,
                total_source,
                target_bytes,
                quality,
                5,
            )
            .unwrap_or((resize_percent, quality))
        }
        None => (resize_percent, quality),
    };

    let mut plan = Vec::with_capacity(collect.files.len());
    let mut warnings = 0usize;
    let mut used_destinations: HashSet<String> = HashSet::new();

    for source in &collect.files {
        let source_size = fs::metadata(source).map(|m| m.len()).unwrap_or(0);
        let estimated_size =
            estimate_size(source_size, effective_resize_percent, effective_quality);
        let relative = relative_or_portable_absolute(source, collect.input_root.as_deref());
        let base_destination = output_dir.join(relative);

        let (status, reason, destination) = resolve_destination_for_policy(
            &base_destination,
            &mut used_destinations,
            request.conflict_policy.clone(),
        );

        let per_file_target_kb = request.target_size_kb.map(|total_kb| {
            let count = collect.files.len() as u64;
            if count == 0 { total_kb } else { total_kb / count }
        });
        let warning_reason = tolerance_warning(
            source_size,
            estimated_size,
            per_file_target_kb,
            tolerance_percent,
        );
        if warning_reason.is_some() {
            warnings += 1;
        }
        let reason = match (reason, warning_reason) {
            (Some(a), Some(b)) => Some(format!("{}; {}", a, b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };

        plan.push(PlannedCompress {
            source: source.clone(),
            destination,
            source_size,
            estimated_size,
            status,
            reason,
        });
    }

    Ok(CompressPlanState {
        output_dir,
        effective_resize_percent,
        effective_quality,
        target_size_kb: request.target_size_kb,
        tolerance_percent,
        plan,
        warnings,
    })
}

fn preview_response_from_state(state: &CompressPlanState) -> CompressPreviewResponse {
    let mut ready = 0usize;
    let mut skipped = 0usize;
    let items = state
        .plan
        .iter()
        .map(|item| {
            match item.status {
                PreviewStatus::Ready => ready += 1,
                PreviewStatus::Skipped => skipped += 1,
            }
            CompressPreviewItem {
                source_path: item.source.to_string_lossy().to_string(),
                destination_path: item.destination.to_string_lossy().to_string(),
                source_size: item.source_size,
                estimated_size: item.estimated_size,
                status: item.status.clone(),
                reason: item.reason.clone(),
            }
        })
        .collect();

    CompressPreviewResponse {
        output_dir: state.output_dir.to_string_lossy().to_string(),
        effective_resize_percent: state.effective_resize_percent,
        effective_quality: state.effective_quality,
        target_size_kb: state.target_size_kb,
        tolerance_percent: state.tolerance_percent,
        items,
        total: ready + skipped,
        ready,
        skipped,
        warnings: state.warnings,
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

fn estimate_size(source_size: u64, resize_percent: f32, quality: u8) -> u64 {
    let resize_ratio = (resize_percent / 100.0).clamp(0.01, 1.0) as f64;
    let quality_ratio = (quality as f64 / 100.0).clamp(0.01, 1.0);
    let quality_factor = quality_ratio.powf(1.25);
    ((source_size as f64) * resize_ratio * resize_ratio * quality_factor).round() as u64
}

fn tolerance_warning(
    source_size: u64,
    estimated_size: u64,
    target_size_kb: Option<u64>,
    tolerance_percent: f32,
) -> Option<String> {
    let target = target_size_kb?;
    let target_bytes = target.saturating_mul(1024);
    if target_bytes == 0 {
        return None;
    }
    let diff = if estimated_size >= target_bytes {
        estimated_size - target_bytes
    } else {
        target_bytes - estimated_size
    };
    let tolerance = ((target_bytes as f64) * (tolerance_percent as f64 / 100.0)) as u64;
    if diff > tolerance {
        Some(format!(
            "推定サイズが許容範囲外です (元={}B, 推定={}B, 目標={}B, 許容={}%)",
            source_size, estimated_size, target_bytes, tolerance_percent
        ))
    } else {
        None
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
        .unwrap_or("compressed")
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
