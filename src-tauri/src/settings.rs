use crate::error::AppError;
use crate::model::{
    AppSettings, DeleteMode, DeletePattern, ImportConflictPreview, RenameTemplate, ThemeMode,
};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use tauri::{AppHandle, Manager};

const SETTINGS_FILE_NAME: &str = "settings.json";

pub fn load_settings(app: &AppHandle) -> Result<AppSettings, AppError> {
    let path = settings_file_path(app)?;
    if !path.exists() {
        return Ok(AppSettings::default());
    }

    let content = fs::read_to_string(&path).map_err(|e| AppError::Settings(e.to_string()))?;
    serde_json::from_str(&content).map_err(|e| AppError::Settings(e.to_string()))
}

pub fn save_settings(app: &AppHandle, settings: &AppSettings) -> Result<(), AppError> {
    validate_settings(settings)?;
    let path = settings_file_path(app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::Settings(e.to_string()))?;
    }
    let body =
        serde_json::to_string_pretty(settings).map_err(|e| AppError::Settings(e.to_string()))?;
    fs::write(path, body).map_err(|e| AppError::Settings(e.to_string()))
}

pub fn settings_file_path(app: &AppHandle) -> Result<PathBuf, AppError> {
    let mut dir = app
        .path()
        .app_config_dir()
        .map_err(|e| AppError::Settings(e.to_string()))?;
    dir.push(SETTINGS_FILE_NAME);
    Ok(dir)
}

pub fn export_settings_to_path(app: &AppHandle, output_path: &str) -> Result<(), AppError> {
    let settings = load_settings(app)?;
    validate_settings(&settings)?;
    let path = PathBuf::from(output_path.trim());
    if path.as_os_str().is_empty() {
        return Err(AppError::Settings("出力パスが指定されていません".to_string()));
    }
    let body =
        serde_json::to_string_pretty(&settings).map_err(|e| AppError::Settings(e.to_string()))?;
    fs::write(path, body).map_err(|e| AppError::Settings(e.to_string()))
}

pub fn import_settings_from_path(
    app: &AppHandle,
    input_path: &str,
    mode: &str,
    conflict_policy: &str,
) -> Result<AppSettings, AppError> {
    let path = PathBuf::from(input_path.trim());
    if path.as_os_str().is_empty() {
        return Err(AppError::Settings("入力パスが指定されていません".to_string()));
    }
    let body = fs::read_to_string(path).map_err(|e| AppError::Settings(e.to_string()))?;
    let imported: AppSettings =
        serde_json::from_str(&body).map_err(|e| AppError::Settings(e.to_string()))?;
    validate_settings(&imported)?;

    let next = match mode {
        "overwrite" => imported,
        "merge" => merge_settings(&load_settings(app)?, &imported, conflict_policy)?,
        _ => {
            return Err(AppError::Settings(
                "モードは overwrite または merge を指定してください".to_string(),
            ));
        }
    };
    save_settings(app, &next)?;
    Ok(next)
}

pub fn preview_import_conflicts(
    app: &AppHandle,
    input_path: &str,
) -> Result<ImportConflictPreview, AppError> {
    let path = PathBuf::from(input_path.trim());
    if path.as_os_str().is_empty() {
        return Err(AppError::Settings("入力パスが指定されていません".to_string()));
    }
    let body = fs::read_to_string(path).map_err(|e| AppError::Settings(e.to_string()))?;
    let imported: AppSettings =
        serde_json::from_str(&body).map_err(|e| AppError::Settings(e.to_string()))?;
    validate_settings(&imported)?;

    let existing = load_settings(app)?;
    let existing_pattern_names: HashSet<String> = existing
        .delete_patterns
        .iter()
        .map(|pattern| pattern.name.to_ascii_lowercase())
        .collect();

    let mut delete_pattern_names: Vec<String> = imported
        .delete_patterns
        .iter()
        .filter(|pattern| existing_pattern_names.contains(&pattern.name.to_ascii_lowercase()))
        .map(|pattern| pattern.name.clone())
        .collect();
    delete_pattern_names.sort_by(|a, b| a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase()));
    delete_pattern_names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));

    let existing_template_names: HashSet<String> = existing
        .rename_templates
        .iter()
        .map(|t| t.name.to_ascii_lowercase())
        .collect();

    let mut rename_template_names: Vec<String> = imported
        .rename_templates
        .iter()
        .filter(|t| existing_template_names.contains(&t.name.to_ascii_lowercase()))
        .map(|t| t.name.clone())
        .collect();
    rename_template_names.sort_by(|a, b| a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase()));
    rename_template_names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));

    let mut output_directory_keys: Vec<String> = imported
        .output_directories
        .keys()
        .filter(|key| existing.output_directories.contains_key(*key))
        .cloned()
        .collect();
    output_directory_keys.sort();
    output_directory_keys.dedup();

    let theme_conflict = !matches!(imported.theme, ThemeMode::System)
        && std::mem::discriminant(&existing.theme) != std::mem::discriminant(&imported.theme);

    Ok(ImportConflictPreview {
        delete_pattern_names,
        rename_template_names,
        output_directory_keys,
        theme_conflict,
    })
}

pub fn open_settings_folder(app: &AppHandle) -> Result<(), AppError> {
    let path = settings_file_path(app)?;
    let folder = path
        .parent()
        .ok_or_else(|| AppError::Settings("設定フォルダが見つかりません".to_string()))?;
    fs::create_dir_all(folder).map_err(|e| AppError::Settings(e.to_string()))?;

    #[cfg(target_os = "windows")]
    {
        Command::new("explorer")
            .arg(folder)
            .spawn()
            .map_err(|e| AppError::Settings(e.to_string()))?;
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(folder)
            .spawn()
            .map_err(|e| AppError::Settings(e.to_string()))?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open")
            .arg(folder)
            .spawn()
            .map_err(|e| AppError::Settings(e.to_string()))?;
    }
    Ok(())
}

fn validate_settings(settings: &AppSettings) -> Result<(), AppError> {
    let mut names = HashSet::new();
    for pattern in &settings.delete_patterns {
        let name = pattern.name.trim();
        let count = name.chars().count();
        if !(1..=40).contains(&count) {
            return Err(AppError::Settings(
                "削除パターン名は1〜40文字で指定してください".to_string(),
            ));
        }
        let lowered = name.to_ascii_lowercase();
        if !names.insert(lowered) {
            return Err(AppError::Settings(
                "削除パターン名は重複できません（大文字小文字を区別しません）".to_string(),
            ));
        }
        if pattern.extensions.is_empty() {
            return Err(AppError::Settings(
                "削除パターンには拡張子を1つ以上指定してください".to_string(),
            ));
        }
        if matches!(pattern.mode, DeleteMode::Retreat) {
            if pattern
                .retreat_dir
                .as_ref()
                .map(|value| value.trim().is_empty())
                .unwrap_or(true)
            {
                return Err(AppError::Settings(
                    "退避モードでは退避先フォルダの指定が必要です".to_string(),
                ));
            }
        }
    }
    Ok(())
}

fn merge_settings(
    existing: &AppSettings,
    imported: &AppSettings,
    conflict_policy: &str,
) -> Result<AppSettings, AppError> {
    let mut delete_patterns = existing.delete_patterns.clone();
    for pattern in &imported.delete_patterns {
        match find_pattern_index(&delete_patterns, &pattern.name) {
            Some(index) => match conflict_policy {
                "existing" => {}
                "import" => delete_patterns[index] = pattern.clone(),
                "cancel" => {
                    return Err(AppError::Settings(format!(
                        "削除パターン `{}` が競合しています",
                        pattern.name
                    )));
                }
                _ => {
                    return Err(AppError::Settings(
                        "conflictPolicy は existing/import/cancel のいずれかを指定してください".to_string(),
                    ));
                }
            },
            None => delete_patterns.push(pattern.clone()),
        }
    }

    let mut rename_templates = existing.rename_templates.clone();
    for tmpl in &imported.rename_templates {
        match find_template_index(&rename_templates, &tmpl.name) {
            Some(index) => match conflict_policy {
                "existing" => {}
                "import" => rename_templates[index] = tmpl.clone(),
                "cancel" => {
                    return Err(AppError::Settings(format!(
                        "リネームテンプレート `{}` が競合しています",
                        tmpl.name
                    )));
                }
                _ => {
                    return Err(AppError::Settings(
                        "conflictPolicy は existing/import/cancel のいずれかを指定してください"
                            .to_string(),
                    ));
                }
            },
            None => rename_templates.push(tmpl.clone()),
        }
    }

    let output_directories = merge_output_dirs(
        &existing.output_directories,
        &imported.output_directories,
        conflict_policy,
    )?;

    let theme = match conflict_policy {
        "existing" => existing.theme.clone(),
        "import" => imported.theme.clone(),
        "cancel" => {
            if !matches!(imported.theme, ThemeMode::System)
                && std::mem::discriminant(&existing.theme)
                    != std::mem::discriminant(&imported.theme)
            {
                return Err(AppError::Settings("テーマの値が競合しています".to_string()));
            }
            existing.theme.clone()
        }
        _ => {
            return Err(AppError::Settings(
                "conflictPolicy は existing/import/cancel のいずれかを指定してください".to_string(),
            ));
        }
    };

    Ok(AppSettings {
        delete_patterns,
        rename_templates,
        output_directories,
        theme,
    })
}

fn find_pattern_index(values: &[DeletePattern], name: &str) -> Option<usize> {
    values
        .iter()
        .position(|item| item.name.to_ascii_lowercase() == name.to_ascii_lowercase())
}

fn find_template_index(values: &[RenameTemplate], name: &str) -> Option<usize> {
    values
        .iter()
        .position(|item| item.name.to_ascii_lowercase() == name.to_ascii_lowercase())
}

fn merge_output_dirs(
    existing: &HashMap<String, String>,
    imported: &HashMap<String, String>,
    conflict_policy: &str,
) -> Result<HashMap<String, String>, AppError> {
    let mut merged = existing.clone();
    for (key, value) in imported {
        if merged.contains_key(key) {
            match conflict_policy {
                "existing" => {}
                "import" => {
                    merged.insert(key.clone(), value.clone());
                }
                "cancel" => {
                    return Err(AppError::Settings(format!(
                        "出力フォルダキー `{}` が競合しています",
                        key
                    )));
                }
                _ => {
                    return Err(AppError::Settings(
                        "conflictPolicy は existing/import/cancel のいずれかを指定してください".to_string(),
                    ));
                }
            }
        } else {
            merged.insert(key.clone(), value.clone());
        }
    }
    Ok(merged)
}
