use crate::error::AppError;
use crate::fs_atomic::atomic_copy_replace;
use crate::path_norm::safe_canonicalize;
use crate::model::{
    CollisionPolicy, ExecuteStatus, FlattenExecuteDetail, FlattenExecuteResponse,
    FlattenPreviewItem, FlattenPreviewRequest, FlattenPreviewResponse, OperationProgressEvent,
    PreviewStatus,
};
use chrono::Local;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;
use walkdir::WalkDir;

#[derive(Debug, Clone)]
struct PlannedFlatten {
    source: PathBuf,
    destination: PathBuf,
    status: PreviewStatus,
    reason: Option<String>,
}

pub fn preview(request: &FlattenPreviewRequest) -> Result<FlattenPreviewResponse, AppError> {
    let (output_dir, plan, collisions) = build_plan(request)?;
    let mut ready = 0usize;
    let mut skipped = 0usize;
    let items = plan
        .iter()
        .map(|item| {
            match item.status {
                PreviewStatus::Ready => ready += 1,
                PreviewStatus::Skipped => skipped += 1,
            }
            FlattenPreviewItem {
                source_path: item.source.to_string_lossy().to_string(),
                destination_path: item.destination.to_string_lossy().to_string(),
                status: item.status.clone(),
                reason: item.reason.clone(),
            }
        })
        .collect();

    Ok(FlattenPreviewResponse {
        output_dir: output_dir.to_string_lossy().to_string(),
        items,
        total: ready + skipped,
        ready,
        skipped,
        collisions,
    })
}

pub fn execute<FCancel, FProgress>(
    request: &FlattenPreviewRequest,
    is_cancelled: FCancel,
    mut report_progress: FProgress,
) -> Result<FlattenExecuteResponse, AppError>
where
    FCancel: Fn() -> bool,
    FProgress: FnMut(OperationProgressEvent),
{
    let (output_dir, plan, _) = build_plan(request)?;
    fs::create_dir_all(&output_dir)?;

    let total = plan.len();
    let mut details = Vec::with_capacity(total);
    let mut succeeded = 0usize;
    let mut failed = 0usize;
    let mut skipped = 0usize;
    let mut processed = 0usize;
    let mut canceled = false;

    let cancel_requested = Arc::new(AtomicBool::new(false));
    let worker_cancel = Arc::clone(&cancel_requested);
    let (tx, rx) = mpsc::channel::<FlattenExecuteDetail>();
    let worker_plan = plan.clone();

    let worker = std::thread::spawn(move || {
        worker_plan
            .into_par_iter()
            .for_each_with(tx, |sender, item| {
                let detail = execute_one_flatten(&item, worker_cancel.load(Ordering::SeqCst));
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
                    operation: "flatten".to_string(),
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

    report_progress(OperationProgressEvent {
        operation: "flatten".to_string(),
        processed,
        total,
        succeeded,
        failed,
        skipped,
        current_path: None,
        done: true,
        canceled,
    });

    Ok(FlattenExecuteResponse {
        output_dir: output_dir.to_string_lossy().to_string(),
        processed: succeeded + failed + skipped,
        succeeded,
        failed,
        skipped,
        details,
    })
}

fn execute_one_flatten(item: &PlannedFlatten, canceled: bool) -> FlattenExecuteDetail {
    if canceled || matches!(item.status, PreviewStatus::Skipped) {
        return FlattenExecuteDetail {
            source_path: item.source.to_string_lossy().to_string(),
            destination_path: item.destination.to_string_lossy().to_string(),
            status: ExecuteStatus::Skipped,
            reason: if canceled {
                Some("キャンセルされました".to_string())
            } else {
                item.reason.clone()
            },
        };
    }

    if let Some(parent) = item.destination.parent() {
        if let Err(error) = fs::create_dir_all(parent) {
            return FlattenExecuteDetail {
                source_path: item.source.to_string_lossy().to_string(),
                destination_path: item.destination.to_string_lossy().to_string(),
                status: ExecuteStatus::Failed,
                reason: Some(format!("出力先フォルダの作成に失敗しました: {}", error)),
            };
        }
    }

    match atomic_copy_replace(&item.source, &item.destination) {
        Ok(()) => FlattenExecuteDetail {
            source_path: item.source.to_string_lossy().to_string(),
            destination_path: item.destination.to_string_lossy().to_string(),
            status: ExecuteStatus::Succeeded,
            reason: None,
        },
        Err(error) => FlattenExecuteDetail {
            source_path: item.source.to_string_lossy().to_string(),
            destination_path: item.destination.to_string_lossy().to_string(),
            status: ExecuteStatus::Failed,
            reason: Some(error),
        },
    }
}

fn build_plan(
    request: &FlattenPreviewRequest,
) -> Result<(PathBuf, Vec<PlannedFlatten>, usize), AppError> {
    let input_dir = PathBuf::from(request.input_dir.trim());
    if !input_dir.exists() {
        return Err(AppError::InvalidRequest(
            "入力フォルダが存在しません".to_string(),
        ));
    }
    if !input_dir.is_dir() {
        return Err(AppError::InvalidRequest(
            "入力パスはフォルダである必要があります".to_string(),
        ));
    }
    let input_dir = safe_canonicalize(&input_dir).map_err(AppError::from)?;

    let output_dir = resolve_output_dir(&input_dir, request.output_dir.as_deref())?;
    validate_output_dir(&input_dir, &output_dir)?;

    let mut sources: Vec<PathBuf> = WalkDir::new(&input_dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.path().to_path_buf())
        .collect();
    sources.sort_by(|a, b| {
        a.to_string_lossy()
            .to_ascii_lowercase()
            .cmp(&b.to_string_lossy().to_ascii_lowercase())
    });

    if sources.is_empty() {
        return Err(AppError::InvalidRequest(
            "入力フォルダにファイルがありません".to_string(),
        ));
    }

    let mut used_destinations = HashSet::new();
    let mut collisions = 0usize;
    let mut plan = Vec::with_capacity(sources.len());
    for source in sources {
        let file_name = source
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| AppError::InvalidRequest("無効なファイル名です".to_string()))?
            .to_string();
        let base_destination = output_dir.join(&file_name);
        let base_key = base_destination.to_string_lossy().to_ascii_lowercase();
        let is_collision = used_destinations.contains(&base_key) || base_destination.exists();
        if is_collision {
            collisions += 1;
        }

        match request.conflict_policy {
            CollisionPolicy::Overwrite => {
                used_destinations.insert(base_key);
                plan.push(PlannedFlatten {
                    source,
                    destination: base_destination,
                    status: PreviewStatus::Ready,
                    reason: if is_collision {
                        Some("競合ポリシーにより上書きされます".to_string())
                    } else {
                        None
                    },
                });
            }
            CollisionPolicy::Skip => {
                if is_collision {
                    plan.push(PlannedFlatten {
                        source,
                        destination: base_destination,
                        status: PreviewStatus::Skipped,
                        reason: Some("ファイル名の競合によりスキップされました".to_string()),
                    });
                } else {
                    used_destinations.insert(base_key);
                    plan.push(PlannedFlatten {
                        source,
                        destination: base_destination,
                        status: PreviewStatus::Ready,
                        reason: None,
                    });
                }
            }
            CollisionPolicy::Sequence => {
                let destination = uniquify_destination(&base_destination, &mut used_destinations);
                let sequenced = destination != base_destination;
                plan.push(PlannedFlatten {
                    source,
                    destination,
                    status: PreviewStatus::Ready,
                    reason: if sequenced {
                        Some("競合のため連番サフィックスを付与しました".to_string())
                    } else {
                        None
                    },
                });
            }
        }
    }

    // When overwrite policy is used and multiple sources map to the same
    // destination, keep only the last writer (by sorted order) as Ready.
    // Earlier duplicates become Skipped to avoid nondeterministic races
    // during parallel execution.
    if matches!(request.conflict_policy, CollisionPolicy::Overwrite) {
        let mut last_ready: HashMap<String, usize> = HashMap::new();
        for (i, item) in plan.iter().enumerate() {
            if matches!(item.status, PreviewStatus::Ready) {
                let key = item.destination.to_string_lossy().to_ascii_lowercase();
                last_ready.insert(key, i);
            }
        }
        for (i, item) in plan.iter_mut().enumerate() {
            if !matches!(item.status, PreviewStatus::Ready) {
                continue;
            }
            let key = item.destination.to_string_lossy().to_ascii_lowercase();
            if let Some(&last) = last_ready.get(&key) {
                if i < last {
                    item.status = PreviewStatus::Skipped;
                    item.reason =
                        Some("同一出力先の後続ファイルに置き換えられました".to_string());
                }
            }
        }
    }

    Ok((output_dir, plan, collisions))
}

fn resolve_output_dir(input_dir: &Path, output_dir: Option<&str>) -> Result<PathBuf, AppError> {
    if let Some(raw) = output_dir {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(AppError::InvalidRequest(
                "出力先フォルダを指定してください".to_string(),
            ));
        }
        return Ok(PathBuf::from(trimmed));
    }
    let parent = input_dir
        .parent()
        .ok_or_else(|| AppError::InvalidRequest("親フォルダを特定できません".to_string()))?;
    let dirname = input_dir
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| AppError::InvalidRequest("入力フォルダの名前が無効です".to_string()))?;
    let timestamp = Local::now().format("%Y%m%d%H%M%S");
    let base_name = format!("{}_flattened_{}", dirname, timestamp);
    let candidate = parent.join(base_name);
    Ok(uniquify_directory(candidate))
}

fn validate_output_dir(input_dir: &Path, output_dir: &Path) -> Result<(), AppError> {
    let output_canonical = safe_canonicalize(output_dir)
        .unwrap_or_else(|_| output_dir.to_path_buf());
    if output_canonical == input_dir {
        return Err(AppError::InvalidRequest(
            "出力先フォルダは入力フォルダと同じにできません".to_string(),
        ));
    }
    if output_canonical.starts_with(input_dir) {
        return Err(AppError::InvalidRequest(
            "出力先フォルダは入力フォルダの内部にできません".to_string(),
        ));
    }
    Ok(())
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
        .unwrap_or("flattened")
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
