use chrono::Local;
use std::fs;
use std::path::{Path, PathBuf};

pub fn atomic_write_replace(destination: &Path, bytes: &[u8]) -> Result<(), String> {
    let temp = temp_path_for(destination, "tmpwrite");
    fs::write(&temp, bytes).map_err(|e| format!("一時ファイルの書き込みに失敗しました: {}", e))?;
    atomic_replace(&temp, destination).map_err(|e| {
        let _ = fs::remove_file(&temp);
        e
    })
}

pub fn atomic_copy_replace(source: &Path, destination: &Path) -> Result<(), String> {
    let temp = temp_path_for(destination, "tmpcopy");
    fs::copy(source, &temp).map_err(|e| format!("一時ファイルへのコピーに失敗しました: {}", e))?;
    atomic_replace(&temp, destination).map_err(|e| {
        let _ = fs::remove_file(&temp);
        e
    })
}

pub fn atomic_move_replace(source: &Path, destination: &Path) -> Result<Option<String>, String> {
    if source == destination {
        return Ok(Some("変更なし".to_string()));
    }

    if !destination.exists() {
        match fs::rename(source, destination) {
            Ok(_) => return Ok(None),
            Err(rename_error) => {
                atomic_copy_replace(source, destination)?;
                fs::remove_file(source).map_err(|remove_error| {
                    format!(
                        "リネーム失敗: {}; コピー後の元ファイル削除にも失敗しました: {}",
                        rename_error, remove_error
                    )
                })?;
                return Ok(Some("コピー+置換のフォールバックで移動しました".to_string()));
            }
        }
    }

    atomic_copy_replace(source, destination)?;
    fs::remove_file(source)
        .map_err(|e| format!("置換は成功しましたが元ファイルの削除に失敗しました: {}", e))?;
    Ok(Some("コピー+置換で移動しました".to_string()))
}

fn temp_path_for(destination: &Path, tag: &str) -> PathBuf {
    let mut temp = destination.to_path_buf();
    let ext = destination
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    let suffix = Local::now().timestamp_nanos_opt().unwrap_or(0);
    let temp_ext = if ext.is_empty() {
        format!("{}_{}", tag, suffix)
    } else {
        format!("{}.{}_{}", ext, tag, suffix)
    };
    temp.set_extension(temp_ext);
    temp
}

fn atomic_replace(temp: &Path, destination: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        if destination.exists() {
            return replace_file_windows(destination, temp);
        }
    }
    fs::rename(temp, destination).map_err(|e| format!("一時ファイルの移動に失敗しました: {}", e))
}

#[cfg(target_os = "windows")]
fn replace_file_windows(destination: &Path, replacement: &Path) -> Result<(), String> {
    use std::ffi::OsStr;
    use std::iter;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::ReplaceFileW;

    fn wide(value: &OsStr) -> Vec<u16> {
        value.encode_wide().chain(iter::once(0)).collect()
    }

    let destination_w = wide(destination.as_os_str());
    let replacement_w = wide(replacement.as_os_str());
    let result = unsafe {
        ReplaceFileW(
            destination_w.as_ptr(),
            replacement_w.as_ptr(),
            std::ptr::null(),
            0,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if result == 0 {
        return Err(format!(
            "ReplaceFileW に失敗しました: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}
