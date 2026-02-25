use serde::{Deserialize, Serialize};

// ── Requests (stdin → worker) ──

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    SampleEstimate {
        id: String,
        files: Vec<String>,
        resize_percent: f32,
        quality: u8,
        max_samples: usize,
    },
    SuggestParams {
        id: String,
        files: Vec<String>,
        total_source_bytes: u64,
        target_bytes: u64,
        quality_seed: u8,
        max_samples: usize,
    },
    CompressBatch {
        id: String,
        items: Vec<CompressBatchItem>,
        resize_percent: f32,
        quality: u8,
        preserve_exif: bool,
    },
    Cancel {
        id: String,
    },
    Shutdown {
        id: String,
    },
}

#[derive(Debug, Deserialize)]
pub struct CompressBatchItem {
    pub source: String,
    pub destination: String,
    pub skip: bool,
}

impl Request {
    pub fn id(&self) -> &str {
        match self {
            Request::SampleEstimate { id, .. } => id,
            Request::SuggestParams { id, .. } => id,
            Request::CompressBatch { id, .. } => id,
            Request::Cancel { id, .. } => id,
            Request::Shutdown { id, .. } => id,
        }
    }
}

// ── Responses (worker → stdout) ──

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Progress {
        id: String,
        current: usize,
        total: usize,
    },
    SampleEstimateResult {
        id: String,
        compression_ratio: f64,
    },
    SuggestParamsResult {
        id: String,
        resize_percent: f32,
        quality: u8,
    },
    CompressFileDone {
        id: String,
        source: String,
        destination: String,
        status: CompressFileStatus,
        output_size: Option<u64>,
        reason: Option<String>,
    },
    CompressBatchDone {
        id: String,
        succeeded: usize,
        failed: usize,
        skipped: usize,
    },
    Error {
        id: String,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompressFileStatus {
    Succeeded,
    Failed,
    Skipped,
}
