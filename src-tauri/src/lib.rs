mod compress;
mod delete;
mod error;
mod exif_offset;
mod file_collect;
mod flatten;
mod fs_atomic;
mod metadata_strip;
mod model;
mod path_norm;
mod rename;
mod settings;
mod worker_bridge;

use crate::error::AppError;
use crate::model::{
    AppSettings, CompressCollectInfoResponse, CompressEstimateResponse, CompressExecuteResponse,
    CompressPreviewRequest, CompressPreviewResponse, DeleteExecuteResponse, DeletePreviewRequest,
    DeletePreviewResponse, ExifOffsetExecuteResponse, ExifOffsetPreviewRequest,
    ExifOffsetPreviewResponse, FlattenExecuteResponse, FlattenPreviewRequest,
    FlattenPreviewResponse, ImportConflictPreview, MetadataStripExecuteResponse,
    MetadataStripPreviewRequest, MetadataStripPreviewResponse, RenameExecuteResponse,
    RenamePreviewRequest, RenamePreviewResponse, RenameTemplateTag,
};
use once_cell::sync::Lazy;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tauri::AppHandle;
use tauri::Emitter;

static CANCEL_REQUESTED: Lazy<AtomicBool> = Lazy::new(|| AtomicBool::new(false));
static ESTIMATE_GENERATION: Lazy<AtomicU64> = Lazy::new(|| AtomicU64::new(0));

#[tauri::command]
fn preview_rename(
    app: AppHandle,
    request: RenamePreviewRequest,
) -> Result<RenamePreviewResponse, String> {
    rename::preview(&request, |event| {
        let _ = app.emit("operation-progress", event);
    })
    .map_err(error_to_string)
}

#[tauri::command]
fn execute_rename(
    app: AppHandle,
    request: RenamePreviewRequest,
) -> Result<RenameExecuteResponse, String> {
    CANCEL_REQUESTED.store(false, Ordering::SeqCst);
    rename::execute(
        &request,
        || CANCEL_REQUESTED.load(Ordering::SeqCst),
        |event| {
            let _ = app.emit("operation-progress", event);
        },
    )
    .map_err(error_to_string)
}

#[tauri::command]
fn preview_delete(request: DeletePreviewRequest) -> Result<DeletePreviewResponse, String> {
    delete::preview(&request).map_err(error_to_string)
}

#[tauri::command]
fn execute_delete(
    app: AppHandle,
    request: DeletePreviewRequest,
) -> Result<DeleteExecuteResponse, String> {
    CANCEL_REQUESTED.store(false, Ordering::SeqCst);
    delete::execute(
        &request,
        || CANCEL_REQUESTED.load(Ordering::SeqCst),
        |event| {
            let _ = app.emit("operation-progress", event);
        },
    )
    .map_err(error_to_string)
}

#[tauri::command]
fn preview_flatten(request: FlattenPreviewRequest) -> Result<FlattenPreviewResponse, String> {
    flatten::preview(&request).map_err(error_to_string)
}

#[tauri::command]
fn execute_flatten(
    app: AppHandle,
    request: FlattenPreviewRequest,
) -> Result<FlattenExecuteResponse, String> {
    CANCEL_REQUESTED.store(false, Ordering::SeqCst);
    flatten::execute(
        &request,
        || CANCEL_REQUESTED.load(Ordering::SeqCst),
        |event| {
            let _ = app.emit("operation-progress", event);
        },
    )
    .map_err(error_to_string)
}

#[tauri::command]
fn compress_collect_info(
    input_paths: Vec<String>,
    include_subfolders: bool,
) -> Result<CompressCollectInfoResponse, String> {
    compress::collect_info(&input_paths, include_subfolders).map_err(error_to_string)
}

#[tauri::command]
async fn compress_estimate(
    app: AppHandle,
    input_paths: Vec<String>,
    include_subfolders: bool,
    resize_percent: f32,
    quality: u8,
) -> Result<CompressEstimateResponse, String> {
    let gen = ESTIMATE_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    tauri::async_runtime::spawn_blocking(move || {
        compress::estimate(
            &app,
            &input_paths,
            include_subfolders,
            resize_percent,
            quality,
            || ESTIMATE_GENERATION.load(Ordering::SeqCst) != gen,
            |event| {
                let _ = app.emit("compress-estimate-progress", event);
            },
        )
        .map_err(error_to_string)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn preview_compress(
    app: AppHandle,
    request: CompressPreviewRequest,
) -> Result<CompressPreviewResponse, String> {
    tauri::async_runtime::spawn_blocking(move || {
        compress::preview(&request, &app).map_err(error_to_string)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn execute_compress(
    app: AppHandle,
    request: CompressPreviewRequest,
) -> Result<CompressExecuteResponse, String> {
    CANCEL_REQUESTED.store(false, Ordering::SeqCst);
    tauri::async_runtime::spawn_blocking(move || {
        compress::execute(
            &app,
            &request,
            || CANCEL_REQUESTED.load(Ordering::SeqCst),
            |event| {
                let _ = app.emit("operation-progress", event);
            },
        )
        .map_err(error_to_string)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn preview_exif_offset(
    request: ExifOffsetPreviewRequest,
) -> Result<ExifOffsetPreviewResponse, String> {
    exif_offset::preview(&request).map_err(error_to_string)
}

#[tauri::command]
fn execute_exif_offset(
    app: AppHandle,
    request: ExifOffsetPreviewRequest,
) -> Result<ExifOffsetExecuteResponse, String> {
    CANCEL_REQUESTED.store(false, Ordering::SeqCst);
    exif_offset::execute(
        &request,
        || CANCEL_REQUESTED.load(Ordering::SeqCst),
        |event| {
            let _ = app.emit("operation-progress", event);
        },
    )
    .map_err(error_to_string)
}

#[tauri::command]
fn preview_metadata_strip(
    request: MetadataStripPreviewRequest,
) -> Result<MetadataStripPreviewResponse, String> {
    metadata_strip::preview(&request).map_err(error_to_string)
}

#[tauri::command]
fn execute_metadata_strip(
    app: AppHandle,
    request: MetadataStripPreviewRequest,
) -> Result<MetadataStripExecuteResponse, String> {
    CANCEL_REQUESTED.store(false, Ordering::SeqCst);
    metadata_strip::execute(
        &request,
        || CANCEL_REQUESTED.load(Ordering::SeqCst),
        |event| {
            let _ = app.emit("operation-progress", event);
        },
    )
    .map_err(error_to_string)
}

#[tauri::command]
fn cancel_operation() {
    CANCEL_REQUESTED.store(true, Ordering::SeqCst);
}

#[tauri::command]
fn is_ffprobe_available() -> bool {
    rename::is_ffprobe_available()
}

#[tauri::command]
fn list_rename_template_tags() -> Vec<RenameTemplateTag> {
    rename::template_tags()
}

#[tauri::command]
fn load_settings(app: AppHandle) -> Result<AppSettings, String> {
    settings::load_settings(&app).map_err(error_to_string)
}

#[tauri::command]
fn save_settings(app: AppHandle, settings: AppSettings) -> Result<(), String> {
    settings::save_settings(&app, &settings).map_err(error_to_string)
}

#[tauri::command]
fn get_settings_path(app: AppHandle) -> Result<String, String> {
    settings::settings_file_path(&app)
        .map(|path| path.to_string_lossy().to_string())
        .map_err(error_to_string)
}

#[tauri::command]
fn export_settings(app: AppHandle, output_path: String) -> Result<(), String> {
    settings::export_settings_to_path(&app, &output_path).map_err(error_to_string)
}

#[tauri::command]
fn import_settings(
    app: AppHandle,
    input_path: String,
    mode: String,
    conflict_policy: String,
) -> Result<AppSettings, String> {
    settings::import_settings_from_path(&app, &input_path, &mode, &conflict_policy)
        .map_err(error_to_string)
}

#[tauri::command]
fn preview_import_conflicts(
    app: AppHandle,
    input_path: String,
) -> Result<ImportConflictPreview, String> {
    settings::preview_import_conflicts(&app, &input_path).map_err(error_to_string)
}

#[tauri::command]
fn open_settings_folder(app: AppHandle) -> Result<(), String> {
    settings::open_settings_folder(&app).map_err(error_to_string)
}

#[tauri::command]
fn is_directory_path(path: String) -> bool {
    std::path::Path::new(path.trim()).is_dir()
}

fn error_to_string(error: AppError) -> String {
    error.to_string()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            preview_rename,
            execute_rename,
            preview_delete,
            execute_delete,
            preview_flatten,
            execute_flatten,
            compress_collect_info,
            compress_estimate,
            preview_compress,
            execute_compress,
            preview_exif_offset,
            execute_exif_offset,
            preview_metadata_strip,
            execute_metadata_strip,
            cancel_operation,
            is_ffprobe_available,
            list_rename_template_tags,
            load_settings,
            save_settings,
            get_settings_path,
            export_settings,
            import_settings,
            preview_import_conflicts,
            open_settings_folder,
            is_directory_path
        ])
        .on_window_event(|_window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                worker_bridge::shutdown();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
