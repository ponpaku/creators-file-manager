mod codec;
mod protocol;

use protocol::{CompressBatchItem, CompressFileStatus, Request, Response};
use rayon::prelude::*;
use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

fn main() {
    let stdout = Arc::new(Mutex::new(io::stdout()));

    // Map of request id → cancel flag
    let cancel_flags: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>> =
        Arc::new(Mutex::new(HashMap::new()));

    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        let request: Request = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let resp = Response::Error {
                    id: String::new(),
                    message: format!("JSON parse error: {}", e),
                };
                send_response(&stdout, &resp);
                continue;
            }
        };

        match &request {
            Request::Shutdown { .. } => {
                break;
            }
            Request::Cancel { id } => {
                let flags = cancel_flags.lock().unwrap();
                if let Some(flag) = flags.get(id) {
                    flag.store(true, Ordering::SeqCst);
                }
                continue;
            }
            _ => {}
        }

        // Register cancel flag for this request
        let cancel_flag = Arc::new(AtomicBool::new(false));
        {
            let mut flags = cancel_flags.lock().unwrap();
            flags.insert(request.id().to_string(), Arc::clone(&cancel_flag));
        }

        let stdout_clone = Arc::clone(&stdout);
        let cancel_flags_clone = Arc::clone(&cancel_flags);

        // Process each request in a dedicated thread so stdin reading continues
        std::thread::spawn(move || {
            handle_request(request, &stdout_clone, &cancel_flag);
            // Clean up cancel flag
            let _ = cancel_flags_clone;
        });
    }
}

fn handle_request(
    request: Request,
    stdout: &Arc<Mutex<io::Stdout>>,
    cancel_flag: &Arc<AtomicBool>,
) {
    match request {
        Request::SampleEstimate {
            id,
            files,
            resize_percent,
            quality,
            max_samples,
        } => {
            handle_sample_estimate(
                &id,
                &files,
                resize_percent,
                quality,
                max_samples,
                stdout,
                cancel_flag,
            );
        }
        Request::SuggestParams {
            id,
            files,
            total_source_bytes,
            target_bytes,
            quality_seed,
            max_samples,
        } => {
            handle_suggest_params(
                &id,
                &files,
                total_source_bytes,
                target_bytes,
                quality_seed,
                max_samples,
                stdout,
                cancel_flag,
            );
        }
        Request::CompressBatch {
            id,
            items,
            resize_percent,
            quality,
            preserve_exif,
        } => {
            handle_compress_batch(
                &id,
                &items,
                resize_percent,
                quality,
                preserve_exif,
                stdout,
                cancel_flag,
            );
        }
        Request::Cancel { .. } | Request::Shutdown { .. } => unreachable!(),
    }
}

fn handle_sample_estimate(
    id: &str,
    files: &[String],
    resize_percent: f32,
    quality: u8,
    max_samples: usize,
    stdout: &Arc<Mutex<io::Stdout>>,
    cancel_flag: &Arc<AtomicBool>,
) {
    let step = if files.len() <= max_samples {
        1
    } else {
        files.len() / max_samples
    };
    let samples: Vec<_> = files
        .iter()
        .step_by(step.max(1))
        .take(max_samples)
        .collect();
    let total = samples.len();
    let done = AtomicUsize::new(0);

    let results: Vec<(u64, u64)> = samples
        .par_iter()
        .filter_map(|path| {
            if cancel_flag.load(Ordering::Relaxed) {
                return None;
            }
            let result =
                codec::sample_compress_in_memory(&PathBuf::from(path), resize_percent, quality);
            let current = done.fetch_add(1, Ordering::Relaxed) + 1;
            send_response(
                stdout,
                &Response::Progress {
                    id: id.to_string(),
                    current,
                    total,
                },
            );
            result
        })
        .collect();

    let (src_total, comp_total) = results
        .iter()
        .fold((0u64, 0u64), |(s, c), &(src, comp)| (s + src, c + comp));
    let compression_ratio = if src_total == 0 {
        1.0
    } else {
        comp_total as f64 / src_total as f64
    };

    send_response(
        stdout,
        &Response::SampleEstimateResult {
            id: id.to_string(),
            compression_ratio,
        },
    );
}

fn handle_suggest_params(
    id: &str,
    files: &[String],
    total_source_bytes: u64,
    target_bytes: u64,
    quality_seed: u8,
    max_samples: usize,
    stdout: &Arc<Mutex<io::Stdout>>,
    cancel_flag: &Arc<AtomicBool>,
) {
    let target = target_bytes as f64;
    if total_source_bytes == 0 || target <= 1.0 {
        send_response(
            stdout,
            &Response::SuggestParamsResult {
                id: id.to_string(),
                resize_percent: 100.0,
                quality: quality_seed.clamp(1, 100),
            },
        );
        return;
    }

    let quality = quality_seed.clamp(20, 95);

    // Select sample files
    let step = if files.len() <= max_samples {
        1
    } else {
        files.len() / max_samples
    };
    let sample_paths: Vec<PathBuf> = files
        .iter()
        .step_by(step.max(1))
        .take(max_samples)
        .map(PathBuf::from)
        .collect();

    // Optimized: decode once, then binary search with encode-only
    // Decode at 100% first (we'll resize at different percentages during search)
    // Actually, resize depends on the percentage, so we must decode once and
    // resize per iteration. But we can cache the decode.
    // Strategy: decode raw images once, then for each binary search iteration,
    // resize from the decoded images + encode.

    // Decode all samples at full resolution
    let decoded: Vec<(u64, image::DynamicImage)> = sample_paths
        .par_iter()
        .filter_map(|path| {
            if cancel_flag.load(Ordering::Relaxed) {
                return None;
            }
            codec::decode_and_resize(path, 100.0) // decode at full size
        })
        .collect();

    if decoded.is_empty() {
        send_response(
            stdout,
            &Response::SuggestParamsResult {
                id: id.to_string(),
                resize_percent: 100.0,
                quality,
            },
        );
        return;
    }

    // Binary search for best resize_percent using cached decoded images
    let mut low: f32 = 10.0;
    let mut high: f32 = 100.0;

    for iteration in 0..5 {
        if cancel_flag.load(Ordering::Relaxed) {
            break;
        }
        let mid = (low + high) / 2.0;
        let ratio = sample_ratio_from_decoded(&decoded, mid, quality);
        let estimated = (total_source_bytes as f64) * ratio;

        send_response(
            stdout,
            &Response::Progress {
                id: id.to_string(),
                current: iteration + 1,
                total: 5,
            },
        );

        if estimated <= target {
            low = mid;
        } else {
            high = mid;
        }
    }

    // Verify low actually fits
    let ratio = sample_ratio_from_decoded(&decoded, low, quality);
    let estimated = (total_source_bytes as f64) * ratio;
    let mut final_quality = quality;

    if estimated > target && low <= 11.0 {
        let needed_ratio = target / (total_source_bytes as f64);
        let current_ratio = ratio;
        if current_ratio > 0.0 {
            let scale = needed_ratio / current_ratio;
            final_quality = ((quality as f64) * scale).round().clamp(10.0, 95.0) as u8;
        }
    }

    send_response(
        stdout,
        &Response::SuggestParamsResult {
            id: id.to_string(),
            resize_percent: low.round().max(10.0),
            quality: final_quality,
        },
    );
}

/// Compute compression ratio from pre-decoded images by resizing + encoding.
fn sample_ratio_from_decoded(
    decoded: &[(u64, image::DynamicImage)],
    resize_percent: f32,
    quality: u8,
) -> f64 {
    let results: Vec<(u64, u64)> = decoded
        .par_iter()
        .filter_map(|(source_size, image)| {
            let ratio = (resize_percent / 100.0).clamp(0.01, 1.0);
            let resized = if ratio < 0.999 {
                let nw = ((image.width() as f32) * ratio).round().max(1.0) as u32;
                let nh = ((image.height() as f32) * ratio).round().max(1.0) as u32;
                image.resize_exact(nw, nh, image::imageops::FilterType::Lanczos3)
            } else {
                image.clone()
            };
            let compressed_size = codec::encode_only(&resized, quality)?;
            Some((*source_size, compressed_size))
        })
        .collect();

    let (src_total, comp_total) = results
        .iter()
        .fold((0u64, 0u64), |(s, c), &(src, comp)| (s + src, c + comp));
    if src_total == 0 {
        1.0
    } else {
        comp_total as f64 / src_total as f64
    }
}

fn handle_compress_batch(
    id: &str,
    items: &[CompressBatchItem],
    resize_percent: f32,
    quality: u8,
    preserve_exif: bool,
    stdout: &Arc<Mutex<io::Stdout>>,
    cancel_flag: &Arc<AtomicBool>,
) {
    let succeeded = AtomicUsize::new(0);
    let failed = AtomicUsize::new(0);
    let skipped = AtomicUsize::new(0);

    let id_owned = id.to_string();
    let stdout_ref = stdout;

    items.par_iter().for_each(|item| {
        if item.skip || cancel_flag.load(Ordering::Relaxed) {
            skipped.fetch_add(1, Ordering::Relaxed);
            send_response(
                stdout_ref,
                &Response::CompressFileDone {
                    id: id_owned.clone(),
                    source: item.source.clone(),
                    destination: item.destination.clone(),
                    status: CompressFileStatus::Skipped,
                    output_size: None,
                    reason: if cancel_flag.load(Ordering::Relaxed) {
                        Some("キャンセルされました".to_string())
                    } else {
                        Some("スキップ".to_string())
                    },
                },
            );
            return;
        }

        let source = PathBuf::from(&item.source);
        let destination = PathBuf::from(&item.destination);

        match codec::compress_one_file(&source, &destination, resize_percent, quality, preserve_exif)
        {
            Ok(size) => {
                succeeded.fetch_add(1, Ordering::Relaxed);
                send_response(
                    stdout_ref,
                    &Response::CompressFileDone {
                        id: id_owned.clone(),
                        source: item.source.clone(),
                        destination: item.destination.clone(),
                        status: CompressFileStatus::Succeeded,
                        output_size: Some(size),
                        reason: None,
                    },
                );
            }
            Err(msg) => {
                failed.fetch_add(1, Ordering::Relaxed);
                send_response(
                    stdout_ref,
                    &Response::CompressFileDone {
                        id: id_owned.clone(),
                        source: item.source.clone(),
                        destination: item.destination.clone(),
                        status: CompressFileStatus::Failed,
                        output_size: None,
                        reason: Some(msg),
                    },
                );
            }
        }
    });

    send_response(
        stdout_ref,
        &Response::CompressBatchDone {
            id: id.to_string(),
            succeeded: succeeded.load(Ordering::Relaxed),
            failed: failed.load(Ordering::Relaxed),
            skipped: skipped.load(Ordering::Relaxed),
        },
    );
}

fn send_response(stdout: &Arc<Mutex<io::Stdout>>, response: &Response) {
    if let Ok(json) = serde_json::to_string(response) {
        if let Ok(mut out) = stdout.lock() {
            let _ = writeln!(out, "{}", json);
            let _ = out.flush();
        }
    }
}
