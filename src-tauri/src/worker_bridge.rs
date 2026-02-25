use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use tauri::AppHandle;
use tauri_plugin_shell::process::CommandChild;
use tauri_plugin_shell::ShellExt;

// ── IPC Message Types (mirror of compress-worker/src/protocol.rs) ──

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerRequest {
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
        items: Vec<CompressBatchItemMsg>,
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

#[derive(Debug, Serialize)]
pub struct CompressBatchItemMsg {
    pub source: String,
    pub destination: String,
    pub skip: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerResponse {
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
        status: String,
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

impl WorkerResponse {
    pub fn id(&self) -> &str {
        match self {
            WorkerResponse::Progress { id, .. } => id,
            WorkerResponse::SampleEstimateResult { id, .. } => id,
            WorkerResponse::SuggestParamsResult { id, .. } => id,
            WorkerResponse::CompressFileDone { id, .. } => id,
            WorkerResponse::CompressBatchDone { id, .. } => id,
            WorkerResponse::Error { id, .. } => id,
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            WorkerResponse::SampleEstimateResult { .. }
                | WorkerResponse::SuggestParamsResult { .. }
                | WorkerResponse::CompressBatchDone { .. }
                | WorkerResponse::Error { .. }
        )
    }
}

// ── WorkerBridge ──

struct WorkerInner {
    child: CommandChild,
    pending: Arc<Mutex<HashMap<String, mpsc::Sender<WorkerResponse>>>>,
    next_id: AtomicU64,
}

static BRIDGE: Mutex<Option<WorkerInner>> = Mutex::new(None);

fn ensure_worker(app: &AppHandle) -> Result<(), String> {
    let mut guard = BRIDGE.lock().map_err(|e| e.to_string())?;
    if guard.is_some() {
        return Ok(());
    }

    let shell = app.shell();
    let (mut rx, child) = shell
        .sidecar("cf-compress-engine")
        .map_err(|e| format!("sidecar コマンド作成に失敗: {}", e))?
        .spawn()
        .map_err(|e| format!("ワーカープロセスの起動に失敗: {}", e))?;

    let pending: Arc<Mutex<HashMap<String, mpsc::Sender<WorkerResponse>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let pending_clone = Arc::clone(&pending);

    // Background thread to read stdout and route responses
    std::thread::spawn(move || {
        use tauri_plugin_shell::process::CommandEvent;
        loop {
            match rx.blocking_recv() {
                Some(CommandEvent::Stdout(line_bytes)) => {
                    let line = String::from_utf8_lossy(&line_bytes);
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    if let Ok(resp) = serde_json::from_str::<WorkerResponse>(line) {
                        let id = resp.id().to_string();
                        let is_terminal = resp.is_terminal();
                        let map = pending_clone.lock().unwrap();
                        if let Some(sender) = map.get(&id) {
                            let _ = sender.send(resp);
                        }
                        drop(map);
                        if is_terminal {
                            let mut map = pending_clone.lock().unwrap();
                            map.remove(&id);
                        }
                    }
                }
                Some(CommandEvent::Terminated(_)) | None => {
                    // Worker crashed or exited — clear all pending
                    let mut map = pending_clone.lock().unwrap();
                    map.clear();
                    break;
                }
                _ => {}
            }
        }
    });

    *guard = Some(WorkerInner {
        child,
        pending,
        next_id: AtomicU64::new(1),
    });

    Ok(())
}

fn send_request(
    request: &WorkerRequest,
) -> Result<mpsc::Receiver<WorkerResponse>, String> {
    let mut guard = BRIDGE.lock().map_err(|e| e.to_string())?;
    let inner = guard
        .as_mut()
        .ok_or_else(|| "ワーカーが起動していません".to_string())?;

    let json =
        serde_json::to_string(request).map_err(|e| format!("リクエストのシリアライズに失敗: {}", e))?;

    let id = request_id(request);
    let (tx, rx) = mpsc::channel();
    {
        let mut pending = inner.pending.lock().unwrap();
        pending.insert(id.to_string(), tx);
    }

    inner
        .child
        .write((json + "\n").as_bytes())
        .map_err(|e| format!("ワーカーへの書き込みに失敗: {}", e))?;

    Ok(rx)
}

fn request_id(req: &WorkerRequest) -> &str {
    match req {
        WorkerRequest::SampleEstimate { id, .. } => id,
        WorkerRequest::SuggestParams { id, .. } => id,
        WorkerRequest::CompressBatch { id, .. } => id,
        WorkerRequest::Cancel { id, .. } => id,
        WorkerRequest::Shutdown { id, .. } => id,
    }
}

fn next_id() -> Result<String, String> {
    let guard = BRIDGE.lock().map_err(|e| e.to_string())?;
    let inner = guard
        .as_ref()
        .ok_or_else(|| "ワーカーが起動していません".to_string())?;
    let id = inner.next_id.fetch_add(1, Ordering::Relaxed);
    Ok(id.to_string())
}

// ── Public API ──

pub fn sample_estimate(
    app: &AppHandle,
    files: Vec<String>,
    resize_percent: f32,
    quality: u8,
    max_samples: usize,
    is_cancelled: impl Fn() -> bool,
    on_progress: impl Fn(usize, usize),
) -> Result<f64, String> {
    ensure_worker(app)?;
    let id = next_id()?;
    let rx = send_request(&WorkerRequest::SampleEstimate {
        id: id.clone(),
        files,
        resize_percent,
        quality,
        max_samples,
    })?;

    loop {
        if is_cancelled() {
            let _ = send_request(&WorkerRequest::Cancel { id: id.clone() });
            return Err("キャンセルされました".to_string());
        }
        match rx.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(WorkerResponse::Progress { current, total, .. }) => {
                on_progress(current, total);
            }
            Ok(WorkerResponse::SampleEstimateResult {
                compression_ratio, ..
            }) => {
                return Ok(compression_ratio);
            }
            Ok(WorkerResponse::Error { message, .. }) => {
                return Err(message);
            }
            Ok(_) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                // Worker died — clear bridge so next call respawns
                let mut guard = BRIDGE.lock().map_err(|e| e.to_string())?;
                *guard = None;
                return Err("ワーカープロセスが予期せず終了しました".to_string());
            }
        }
    }
}

pub fn suggest_params(
    app: &AppHandle,
    files: Vec<String>,
    total_source_bytes: u64,
    target_bytes: u64,
    quality_seed: u8,
    max_samples: usize,
) -> Result<(f32, u8), String> {
    ensure_worker(app)?;
    let id = next_id()?;
    let rx = send_request(&WorkerRequest::SuggestParams {
        id: id.clone(),
        files,
        total_source_bytes,
        target_bytes,
        quality_seed,
        max_samples,
    })?;

    loop {
        match rx.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(WorkerResponse::Progress { .. }) => {}
            Ok(WorkerResponse::SuggestParamsResult {
                resize_percent,
                quality,
                ..
            }) => {
                return Ok((resize_percent, quality));
            }
            Ok(WorkerResponse::Error { message, .. }) => {
                return Err(message);
            }
            Ok(_) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let mut guard = BRIDGE.lock().map_err(|e| e.to_string())?;
                *guard = None;
                return Err("ワーカープロセスが予期せず終了しました".to_string());
            }
        }
    }
}

pub struct BatchProgress {
    pub source: String,
    pub destination: String,
    pub status: String,
    pub output_size: Option<u64>,
    pub reason: Option<String>,
}

pub struct BatchResult {
    pub succeeded: usize,
    pub failed: usize,
    pub skipped: usize,
}

pub fn compress_batch(
    app: &AppHandle,
    items: Vec<CompressBatchItemMsg>,
    resize_percent: f32,
    quality: u8,
    preserve_exif: bool,
    is_cancelled: impl Fn() -> bool,
    on_file_done: impl FnMut(BatchProgress),
) -> Result<BatchResult, String> {
    ensure_worker(app)?;
    let id = next_id()?;
    let rx = send_request(&WorkerRequest::CompressBatch {
        id: id.clone(),
        items,
        resize_percent,
        quality,
        preserve_exif,
    })?;

    let mut on_file_done = on_file_done;

    loop {
        if is_cancelled() {
            let _ = send_request(&WorkerRequest::Cancel { id: id.clone() });
        }
        match rx.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(WorkerResponse::CompressFileDone {
                source,
                destination,
                status,
                output_size,
                reason,
                ..
            }) => {
                on_file_done(BatchProgress {
                    source,
                    destination,
                    status,
                    output_size,
                    reason,
                });
            }
            Ok(WorkerResponse::CompressBatchDone {
                succeeded,
                failed,
                skipped,
                ..
            }) => {
                return Ok(BatchResult {
                    succeeded,
                    failed,
                    skipped,
                });
            }
            Ok(WorkerResponse::Error { message, .. }) => {
                return Err(message);
            }
            Ok(_) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let mut guard = BRIDGE.lock().map_err(|e| e.to_string())?;
                *guard = None;
                return Err("ワーカープロセスが予期せず終了しました".to_string());
            }
        }
    }
}

pub fn shutdown() {
    if let Ok(mut guard) = BRIDGE.lock() {
        if guard.is_some() {
            // Best-effort shutdown
            let id = "shutdown".to_string();
            if let Some(inner) = guard.as_mut() {
                let req = WorkerRequest::Shutdown { id };
                if let Ok(json) = serde_json::to_string(&req) {
                    let _ = inner.child.write((json + "\n").as_bytes());
                }
            }
            *guard = None;
        }
    }
}
