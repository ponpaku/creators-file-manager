import { invoke } from "@tauri-apps/api/core";
import type {
  AppSettings,
  CompressCollectInfoResponse,
  CompressEstimateResponse,
  CompressExecuteResponse,
  CompressPreviewRequest,
  CompressPreviewResponse,
  DeleteExecuteResponse,
  DeletePreviewRequest,
  DeletePreviewResponse,
  ExifOffsetExecuteResponse,
  ExifOffsetPreviewRequest,
  ExifOffsetPreviewResponse,
  FlattenExecuteResponse,
  FlattenPreviewRequest,
  FlattenPreviewResponse,
  ImportConflictPreview,
  MetadataStripExecuteResponse,
  MetadataStripPreviewRequest,
  MetadataStripPreviewResponse,
  RenameExecuteResponse,
  RenamePreviewRequest,
  RenamePreviewResponse,
  RenameTemplateTag
} from "./types";

export async function previewRename(
  payload: RenamePreviewRequest
): Promise<RenamePreviewResponse> {
  return invoke<RenamePreviewResponse>("preview_rename", { request: payload });
}

export async function executeRename(
  payload: RenamePreviewRequest
): Promise<RenameExecuteResponse> {
  return invoke<RenameExecuteResponse>("execute_rename", { request: payload });
}

export async function previewDelete(
  payload: DeletePreviewRequest
): Promise<DeletePreviewResponse> {
  return invoke<DeletePreviewResponse>("preview_delete", { request: payload });
}

export async function executeDelete(
  payload: DeletePreviewRequest
): Promise<DeleteExecuteResponse> {
  return invoke<DeleteExecuteResponse>("execute_delete", { request: payload });
}

export async function previewFlatten(
  payload: FlattenPreviewRequest
): Promise<FlattenPreviewResponse> {
  return invoke<FlattenPreviewResponse>("preview_flatten", { request: payload });
}

export async function executeFlatten(
  payload: FlattenPreviewRequest
): Promise<FlattenExecuteResponse> {
  return invoke<FlattenExecuteResponse>("execute_flatten", { request: payload });
}

export async function compressCollectInfo(
  inputPaths: string[],
  includeSubfolders: boolean
): Promise<CompressCollectInfoResponse> {
  return invoke<CompressCollectInfoResponse>("compress_collect_info", { inputPaths, includeSubfolders });
}

export async function compressEstimate(
  inputPaths: string[],
  includeSubfolders: boolean,
  resizePercent: number,
  quality: number
): Promise<CompressEstimateResponse> {
  return invoke<CompressEstimateResponse>("compress_estimate", {
    inputPaths,
    includeSubfolders,
    resizePercent,
    quality,
  });
}

export async function previewCompress(
  payload: CompressPreviewRequest
): Promise<CompressPreviewResponse> {
  return invoke<CompressPreviewResponse>("preview_compress", { request: payload });
}

export async function executeCompress(
  payload: CompressPreviewRequest
): Promise<CompressExecuteResponse> {
  return invoke<CompressExecuteResponse>("execute_compress", { request: payload });
}

export async function previewExifOffset(
  payload: ExifOffsetPreviewRequest
): Promise<ExifOffsetPreviewResponse> {
  return invoke<ExifOffsetPreviewResponse>("preview_exif_offset", { request: payload });
}

export async function executeExifOffset(
  payload: ExifOffsetPreviewRequest
): Promise<ExifOffsetExecuteResponse> {
  return invoke<ExifOffsetExecuteResponse>("execute_exif_offset", { request: payload });
}

export async function previewMetadataStrip(
  payload: MetadataStripPreviewRequest
): Promise<MetadataStripPreviewResponse> {
  return invoke<MetadataStripPreviewResponse>("preview_metadata_strip", { request: payload });
}

export async function executeMetadataStrip(
  payload: MetadataStripPreviewRequest
): Promise<MetadataStripExecuteResponse> {
  return invoke<MetadataStripExecuteResponse>("execute_metadata_strip", { request: payload });
}

export async function cancelOperation(): Promise<void> {
  await invoke("cancel_operation");
}

export async function loadSettings(): Promise<AppSettings> {
  return invoke<AppSettings>("load_settings");
}

export async function saveSettings(settings: AppSettings): Promise<void> {
  await invoke("save_settings", { settings });
}

export async function getSettingsPath(): Promise<string> {
  return invoke<string>("get_settings_path");
}

export async function exportSettings(outputPath: string): Promise<void> {
  await invoke("export_settings", { outputPath });
}

export async function importSettings(
  inputPath: string,
  mode: "overwrite" | "merge",
  conflictPolicy: "existing" | "import" | "cancel"
): Promise<AppSettings> {
  return invoke<AppSettings>("import_settings", { inputPath, mode, conflictPolicy });
}

export async function previewImportConflicts(
  inputPath: string
): Promise<ImportConflictPreview> {
  return invoke<ImportConflictPreview>("preview_import_conflicts", { inputPath });
}

export async function openSettingsFolder(): Promise<void> {
  await invoke("open_settings_folder");
}

export async function isFfprobeAvailable(): Promise<boolean> {
  return invoke<boolean>("is_ffprobe_available");
}

export async function listRenameTemplateTags(): Promise<RenameTemplateTag[]> {
  return invoke<RenameTemplateTag[]>("list_rename_template_tags");
}

export async function isDirectoryPath(path: string): Promise<boolean> {
  return invoke<boolean>("is_directory_path", { path });
}
