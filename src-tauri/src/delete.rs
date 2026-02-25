use crate::error::AppError;
use crate::file_collect::collect_targets_with_extensions;
use crate::fs_atomic::atomic_move_replace;
use crate::model::{
    CollisionPolicy, DeleteExecuteDetail, DeleteExecuteResponse, DeleteMode, DeletePreviewItem,
    DeletePreviewRequest, DeletePreviewResponse, ExecuteStatus, OperationProgressEvent,
    PreviewStatus,
};
use crate::path_norm::relative_or_portable_absolute;
use std::collections::{HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
struct PlannedDelete {
    source: PathBuf,
    destination: Option<PathBuf>,
    status: PreviewStatus,
    reason: Option<String>,
}

pub fn preview(request: &DeletePreviewRequest) -> Result<DeletePreviewResponse, AppError> {
    let (plan, mode) = build_plan(request)?;
    let mut ready = 0usize;
    let mut skipped = 0usize;

    let items = plan
        .iter()
        .map(|item| {
            match item.status {
                PreviewStatus::Ready => ready += 1,
                PreviewStatus::Skipped => skipped += 1,
            }
            DeletePreviewItem {
                source_path: item.source.to_string_lossy().to_string(),
                action: delete_mode_label(&mode),
                destination_path: item
                    .destination
                    .as_ref()
                    .map(|path| path.to_string_lossy().to_string()),
                status: item.status.clone(),
                reason: item.reason.clone(),
            }
        })
        .collect();

    Ok(DeletePreviewResponse {
        items,
        total: ready + skipped,
        ready,
        skipped,
    })
}

pub fn execute<FCancel, FProgress>(
    request: &DeletePreviewRequest,
    is_cancelled: FCancel,
    mut report_progress: FProgress,
) -> Result<DeleteExecuteResponse, AppError>
where
    FCancel: Fn() -> bool,
    FProgress: FnMut(OperationProgressEvent),
{
    let (plan, mode) = build_plan(request)?;
    let mut details = Vec::with_capacity(plan.len());
    let mut succeeded = 0usize;
    let mut failed = 0usize;
    let mut skipped = 0usize;
    let total = plan.len();
    let mut processed = 0usize;
    let mut canceled = false;

    for item in plan {
        if is_cancelled() {
            canceled = true;
        }

        if canceled || matches!(item.status, PreviewStatus::Skipped) {
            skipped += 1;
            processed += 1;
            details.push(DeleteExecuteDetail {
                source_path: item.source.to_string_lossy().to_string(),
                action: delete_mode_label(&mode),
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
            });
            report_progress(OperationProgressEvent {
                operation: "delete".to_string(),
                processed,
                total,
                succeeded,
                failed,
                skipped,
                current_path: Some(item.source.to_string_lossy().to_string()),
                done: false,
                canceled,
            });
            continue;
        }

        let result = match mode {
            DeleteMode::Direct => fs::remove_file(&item.source)
                .map(|_| None)
                .map_err(|e| format!("ファイルの削除に失敗しました: {}", e)),
            DeleteMode::Trash => trash::delete(&item.source)
                .map(|_| None)
                .map_err(|e| format!("ゴミ箱への移動に失敗しました: {}", e)),
            DeleteMode::Retreat => {
                let Some(destination) = item.destination.as_ref() else {
                    return Err(AppError::InvalidRequest(
                        "退避先が指定されていません".to_string(),
                    ));
                };
                if let Some(parent) = destination.parent() {
                    if let Err(error) = fs::create_dir_all(parent) {
                        Err(format!("出力先フォルダの作成に失敗しました: {}", error))
                    } else {
                        atomic_move_replace(&item.source, destination)
                    }
                } else {
                    atomic_move_replace(&item.source, destination)
                }
            }
        };

        match result {
            Ok(note) => {
                succeeded += 1;
                processed += 1;
                details.push(DeleteExecuteDetail {
                    source_path: item.source.to_string_lossy().to_string(),
                    action: delete_mode_label(&mode),
                    destination_path: item
                        .destination
                        .as_ref()
                        .map(|path| path.to_string_lossy().to_string()),
                    status: ExecuteStatus::Succeeded,
                    reason: note,
                });
            }
            Err(error) => {
                failed += 1;
                processed += 1;
                details.push(DeleteExecuteDetail {
                    source_path: item.source.to_string_lossy().to_string(),
                    action: delete_mode_label(&mode),
                    destination_path: item
                        .destination
                        .as_ref()
                        .map(|path| path.to_string_lossy().to_string()),
                    status: ExecuteStatus::Failed,
                    reason: Some(error),
                });
            }
        }

        report_progress(OperationProgressEvent {
            operation: "delete".to_string(),
            processed,
            total,
            succeeded,
            failed,
            skipped,
            current_path: Some(item.source.to_string_lossy().to_string()),
            done: false,
            canceled,
        });
    }

    report_progress(OperationProgressEvent {
        operation: "delete".to_string(),
        processed: total,
        total,
        succeeded,
        failed,
        skipped,
        current_path: None,
        done: true,
        canceled,
    });

    Ok(DeleteExecuteResponse {
        processed: succeeded + failed + skipped,
        succeeded,
        failed,
        skipped,
        details,
    })
}

fn build_plan(
    request: &DeletePreviewRequest,
) -> Result<(Vec<PlannedDelete>, DeleteMode), AppError> {
    let normalized_extensions = normalize_extensions(&request.extensions)?;
    let refs: Vec<&str> = normalized_extensions.iter().map(String::as_str).collect();
    let collect = collect_targets_with_extensions(
        &request.input_paths,
        request.include_subfolders,
        refs.as_slice(),
    )
    .map_err(AppError::InvalidRequest)?;
    if collect.files.is_empty() {
        return Ok((Vec::new(), request.mode.clone()));
    }

    let retreat_dir = match request.mode {
        DeleteMode::Retreat => {
            let Some(dir) = request.retreat_dir.as_ref() else {
                return Err(AppError::InvalidRequest(
                    "退避モードでは退避先フォルダの指定が必要です".to_string(),
                ));
            };
            let value = dir.trim();
            if value.is_empty() {
                return Err(AppError::InvalidRequest(
                    "退避先フォルダを指定してください".to_string(),
                ));
            }
            Some(PathBuf::from(value))
        }
        _ => None,
    };
    let conflict_policy = request
        .conflict_policy
        .clone()
        .unwrap_or(CollisionPolicy::Sequence);

    let mut used_destinations: HashSet<String> = HashSet::new();
    let mut plan = Vec::with_capacity(collect.files.len());
    for source in &collect.files {
        let (status, reason, destination) = match request.mode {
            DeleteMode::Direct | DeleteMode::Trash => (PreviewStatus::Ready, None, None),
            DeleteMode::Retreat => {
                let retreat_root = retreat_dir
                    .as_ref()
                    .ok_or_else(|| AppError::InvalidRequest("退避先ルートが未設定です".to_string()))?;
                let relative = relative_or_portable_absolute(source, collect.input_root.as_deref());
                let base_destination = retreat_root.join(relative);
                let (status, reason, destination) = resolve_retreat_destination_for_policy(
                    &base_destination,
                    source,
                    &mut used_destinations,
                    &conflict_policy,
                );
                (status, reason, destination)
            }
        };

        plan.push(PlannedDelete {
            source: source.clone(),
            destination,
            status,
            reason,
        });
    }

    Ok((plan, request.mode.clone()))
}

fn normalize_extensions(values: &[String]) -> Result<Vec<String>, AppError> {
    let mut unique = HashSet::new();
    let mut queue = VecDeque::new();
    for raw in values {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let normalized = trimmed
            .trim_start_matches('.')
            .to_ascii_lowercase()
            .replace(' ', "");
        if normalized.is_empty() {
            continue;
        }
        if normalized.contains('.') || normalized.contains('/') || normalized.contains('\\') {
            return Err(AppError::InvalidRequest(format!(
                "無効な拡張子フォーマットです: `{}`",
                raw
            )));
        }
        if unique.insert(normalized.clone()) {
            queue.push_back(normalized);
        }
    }
    if queue.is_empty() {
        return Err(AppError::InvalidRequest(
            "拡張子を1つ以上指定してください".to_string(),
        ));
    }
    Ok(queue.into_iter().collect())
}

fn delete_mode_label(mode: &DeleteMode) -> String {
    match mode {
        DeleteMode::Direct => "direct".to_string(),
        DeleteMode::Trash => "trash".to_string(),
        DeleteMode::Retreat => "retreat".to_string(),
    }
}

fn resolve_retreat_destination_for_policy(
    base: &Path,
    source: &Path,
    used_keys: &mut HashSet<String>,
    policy: &CollisionPolicy,
) -> (PreviewStatus, Option<String>, Option<PathBuf>) {
    let key = base.to_string_lossy().to_ascii_lowercase();
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
                Some(base.to_path_buf()),
            )
        }
        CollisionPolicy::Skip => {
            if collision {
                (
                    PreviewStatus::Skipped,
                    Some("出力先の競合によりスキップされました".to_string()),
                    Some(base.to_path_buf()),
                )
            } else {
                used_keys.insert(key);
                (PreviewStatus::Ready, None, Some(base.to_path_buf()))
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
                Some(unique),
            )
        }
    }
}

fn uniquify_destination(base: &Path, source: &Path, used_keys: &mut HashSet<String>) -> PathBuf {
    let mut candidate = base.to_path_buf();
    let mut suffix = 1usize;

    loop {
        let key = candidate.to_string_lossy().to_ascii_lowercase();
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
