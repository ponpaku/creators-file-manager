use crate::path_norm::safe_canonicalize;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub const RENAME_ALLOWED_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "webp", "gif", "tif", "tiff", "bmp", "heic", "heif", "dng", "cr2", "cr3",
    "nef", "arw", "raf", "mp4", "mov", "m4v", "avi", "mkv", "wmv", "mts", "m2ts", "mpg", "mpeg",
    "webm", "mxf",
];

pub const JPEG_ALLOWED_EXTENSIONS: &[&str] = &["jpg", "jpeg"];

#[derive(Debug, Clone)]
pub struct CollectResult {
    pub files: Vec<PathBuf>,
    pub input_root: Option<PathBuf>,
    pub single_input_root: bool,
    pub skipped_by_extension: usize,
}

pub fn collect_rename_targets(
    input_paths: &[String],
    include_subfolders: bool,
) -> Result<CollectResult, String> {
    collect_targets_with_extensions(input_paths, include_subfolders, RENAME_ALLOWED_EXTENSIONS)
}

pub fn collect_targets_with_extensions(
    input_paths: &[String],
    include_subfolders: bool,
    allowed_extensions: &[&str],
) -> Result<CollectResult, String> {
    if input_paths.is_empty() {
        return Err("入力パスが指定されていません".to_string());
    }

    let mut resolved_inputs: Vec<PathBuf> = Vec::new();
    for raw in input_paths {
        let path = PathBuf::from(raw);
        if !path.exists() {
            return Err(format!("入力パスが存在しません: {}", raw));
        }
        resolved_inputs.push(
            safe_canonicalize(&path)
                .map_err(|e| format!("パスの正規化に失敗しました `{}`: {}", raw, e))?,
        );
    }

    let mut files = BTreeSet::new();
    let mut skipped_by_extension = 0usize;
    for path in &resolved_inputs {
        if path.is_file() {
            if has_allowed_extension(path, allowed_extensions) {
                files.insert(path.clone());
            } else {
                skipped_by_extension += 1;
            }
            continue;
        }
        if path.is_dir() {
            skipped_by_extension += collect_from_dir(
                path,
                include_subfolders,
                allowed_extensions,
                &mut files,
            )?;
        }
    }

    let mut file_list: Vec<PathBuf> = files.into_iter().collect();
    file_list.sort_by(|a, b| {
        a.to_string_lossy()
            .to_lowercase()
            .cmp(&b.to_string_lossy().to_lowercase())
    });

    let input_root = find_common_parent(&file_list);
    Ok(CollectResult {
        files: file_list,
        single_input_root: input_root.is_some(),
        input_root,
        skipped_by_extension,
    })
}

fn collect_from_dir(
    dir: &Path,
    include_subfolders: bool,
    allowed_extensions: &[&str],
    files: &mut BTreeSet<PathBuf>,
) -> Result<usize, String> {
    let mut skipped = 0usize;
    if include_subfolders {
        for entry in WalkDir::new(dir).into_iter() {
            let entry = entry.map_err(|e| format!("フォルダの走査に失敗しました: {}", e))?;
            if entry.file_type().is_file() {
                if has_allowed_extension(entry.path(), allowed_extensions) {
                    files.insert(
                        safe_canonicalize(entry.path())
                            .map_err(|e| format!("パスの正規化に失敗しました: {}", e))?,
                    );
                } else {
                    skipped += 1;
                }
            }
        }
        return Ok(skipped);
    }

    let entries = fs::read_dir(dir).map_err(|e| format!("フォルダの読み込みに失敗しました: {}", e))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("フォルダエントリの読み込みに失敗しました: {}", e))?;
        let path = entry.path();
        if path.is_file() {
            if has_allowed_extension(&path, allowed_extensions) {
                files.insert(
                    safe_canonicalize(&path)
                        .map_err(|e| format!("パスの正規化に失敗しました: {}", e))?,
                );
            } else {
                skipped += 1;
            }
        }
    }
    Ok(skipped)
}

fn has_allowed_extension(path: &Path, allowed: &[&str]) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            let ext = ext.to_ascii_lowercase();
            allowed.iter().any(|item| *item == ext)
        })
        .unwrap_or(false)
}

fn find_common_parent(files: &[PathBuf]) -> Option<PathBuf> {
    if files.is_empty() {
        return None;
    }
    let mut current = files[0].parent()?.to_path_buf();
    for path in files.iter().skip(1) {
        let parent = path.parent()?;
        while !parent.starts_with(&current) {
            if !current.pop() {
                return None;
            }
        }
    }
    Some(current)
}
