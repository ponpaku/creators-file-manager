use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;

/// Backward-compatible deserializer: accepts both Vec<String> (old) and Vec<RenameTemplate> (new).
fn deserialize_rename_templates<'de, D>(deserializer: D) -> Result<Vec<RenameTemplate>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Item {
        Named(RenameTemplate),
        Plain(String),
    }
    let items: Vec<Item> = Vec::deserialize(deserializer)?;
    Ok(items
        .into_iter()
        .map(|item| match item {
            Item::Named(t) => t,
            Item::Plain(s) => RenameTemplate {
                name: s.clone(),
                template: s,
            },
        })
        .collect())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RenameSource {
    CaptureThenModified,
    ModifiedOnly,
    CurrentTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenamePreviewRequest {
    pub input_paths: Vec<String>,
    pub include_subfolders: bool,
    pub template: String,
    pub source: RenameSource,
    pub output_dir: Option<String>,
    pub conflict_policy: Option<CollisionPolicy>,
    pub use_ffprobe: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenamePreviewItem {
    pub source_path: String,
    pub destination_path: Option<String>,
    pub status: PreviewStatus,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PreviewStatus {
    Ready,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenamePreviewResponse {
    pub items: Vec<RenamePreviewItem>,
    pub total: usize,
    pub ready: usize,
    pub skipped: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameExecuteDetail {
    pub source_path: String,
    pub destination_path: Option<String>,
    pub status: ExecuteStatus,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExecuteStatus {
    Succeeded,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameExecuteResponse {
    pub processed: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub skipped: usize,
    pub details: Vec<RenameExecuteDetail>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameTemplateTag {
    pub token: String,
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletePreviewRequest {
    pub input_paths: Vec<String>,
    pub include_subfolders: bool,
    pub extensions: Vec<String>,
    pub mode: DeleteMode,
    pub retreat_dir: Option<String>,
    pub conflict_policy: Option<CollisionPolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletePreviewItem {
    pub source_path: String,
    pub action: String,
    pub destination_path: Option<String>,
    pub status: PreviewStatus,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletePreviewResponse {
    pub items: Vec<DeletePreviewItem>,
    pub total: usize,
    pub ready: usize,
    pub skipped: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteExecuteDetail {
    pub source_path: String,
    pub action: String,
    pub destination_path: Option<String>,
    pub status: ExecuteStatus,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteExecuteResponse {
    pub processed: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub skipped: usize,
    pub details: Vec<DeleteExecuteDetail>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CollisionPolicy {
    Overwrite,
    Sequence,
    Skip,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlattenPreviewRequest {
    pub input_dir: String,
    pub output_dir: Option<String>,
    pub conflict_policy: CollisionPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlattenPreviewItem {
    pub source_path: String,
    pub destination_path: String,
    pub status: PreviewStatus,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlattenPreviewResponse {
    pub output_dir: String,
    pub items: Vec<FlattenPreviewItem>,
    pub total: usize,
    pub ready: usize,
    pub skipped: usize,
    pub collisions: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlattenExecuteDetail {
    pub source_path: String,
    pub destination_path: String,
    pub status: ExecuteStatus,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlattenExecuteResponse {
    pub output_dir: String,
    pub processed: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub skipped: usize,
    pub details: Vec<FlattenExecuteDetail>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompressPreviewRequest {
    pub input_paths: Vec<String>,
    pub include_subfolders: bool,
    pub resize_percent: f32,
    pub quality: u8,
    pub target_size_kb: Option<u64>,
    pub tolerance_percent: Option<f32>,
    pub preserve_exif: bool,
    pub output_dir: Option<String>,
    pub conflict_policy: CollisionPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompressPreviewItem {
    pub source_path: String,
    pub destination_path: String,
    pub source_size: u64,
    pub estimated_size: u64,
    pub status: PreviewStatus,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompressPreviewResponse {
    pub output_dir: String,
    pub effective_resize_percent: f32,
    pub effective_quality: u8,
    pub target_size_kb: Option<u64>,
    pub tolerance_percent: f32,
    pub items: Vec<CompressPreviewItem>,
    pub total: usize,
    pub ready: usize,
    pub skipped: usize,
    pub warnings: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompressExecuteDetail {
    pub source_path: String,
    pub destination_path: String,
    pub status: ExecuteStatus,
    pub output_size: Option<u64>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompressExecuteResponse {
    pub output_dir: String,
    pub effective_resize_percent: f32,
    pub effective_quality: u8,
    pub processed: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub skipped: usize,
    pub details: Vec<CompressExecuteDetail>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationProgressEvent {
    pub operation: String,
    pub processed: usize,
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub skipped: usize,
    pub current_path: Option<String>,
    pub done: bool,
    pub canceled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletePattern {
    pub name: String,
    pub extensions: Vec<String>,
    pub mode: DeleteMode,
    pub retreat_dir: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DeleteMode {
    Direct,
    Trash,
    Retreat,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameTemplate {
    pub name: String,
    pub template: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub delete_patterns: Vec<DeletePattern>,
    #[serde(deserialize_with = "deserialize_rename_templates")]
    pub rename_templates: Vec<RenameTemplate>,
    pub output_directories: HashMap<String, String>,
    pub theme: ThemeMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportConflictPreview {
    pub delete_pattern_names: Vec<String>,
    pub rename_template_names: Vec<String>,
    pub output_directory_keys: Vec<String>,
    pub theme_conflict: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThemeMode {
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompressCollectInfoResponse {
    pub file_count: usize,
    pub total_size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompressEstimateResponse {
    pub file_count: usize,
    pub total_source_size: u64,
    pub estimated_total_size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EstimateProgressEvent {
    pub current: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExifOffsetPreviewRequest {
    pub input_paths: Vec<String>,
    pub include_subfolders: bool,
    pub offset_seconds: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExifOffsetPreviewItem {
    pub source_path: String,
    pub original_datetime: Option<String>,
    pub corrected_datetime: Option<String>,
    pub status: PreviewStatus,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExifOffsetPreviewResponse {
    pub items: Vec<ExifOffsetPreviewItem>,
    pub total: usize,
    pub ready: usize,
    pub skipped: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExifOffsetExecuteDetail {
    pub source_path: String,
    pub status: ExecuteStatus,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExifOffsetExecuteResponse {
    pub processed: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub skipped: usize,
    pub details: Vec<ExifOffsetExecuteDetail>,
}

// ===== Metadata Strip =====

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataStripCategories {
    pub gps: bool,
    pub camera_lens: bool,
    pub software: bool,
    pub author_copyright: bool,
    pub comments: bool,
    pub thumbnail: bool,
    pub iptc: bool,
    pub xmp: bool,
    pub shooting_settings: bool,
    #[serde(rename = "captureDateTime")]
    pub capture_datetime: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MetadataStripPreset {
    SnsPublish,
    Delivery,
    FullClean,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataStripPreviewRequest {
    pub input_paths: Vec<String>,
    pub include_subfolders: bool,
    pub preset: MetadataStripPreset,
    pub categories: MetadataStripCategories,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataStripPreviewItem {
    pub source_path: String,
    pub found_categories: Vec<String>,
    pub tags_to_strip: usize,
    pub has_iptc: bool,
    pub has_xmp: bool,
    pub status: PreviewStatus,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataStripPreviewResponse {
    pub items: Vec<MetadataStripPreviewItem>,
    pub total: usize,
    pub ready: usize,
    pub skipped: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataStripExecuteDetail {
    pub source_path: String,
    pub stripped_tags: usize,
    pub stripped_iptc: bool,
    pub stripped_xmp: bool,
    pub status: ExecuteStatus,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataStripExecuteResponse {
    pub processed: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub skipped: usize,
    pub details: Vec<MetadataStripExecuteDetail>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            delete_patterns: Vec::new(),
            rename_templates: vec![RenameTemplate {
                name: "日付通番".to_string(),
                template: "{capture_date:YYYYMMDD}_{capture_time:HHmmss}_{seq:3}".to_string(),
            }],
            output_directories: HashMap::new(),
            theme: ThemeMode::System,
        }
    }
}
