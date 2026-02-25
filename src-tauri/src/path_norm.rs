use std::path::{Component, Path, PathBuf, Prefix};

/// `canonicalize()` wrapper that strips the Windows `\\?\` prefix.
pub fn safe_canonicalize(path: &Path) -> std::io::Result<PathBuf> {
    let canonical = path.canonicalize()?;
    Ok(strip_verbatim(canonical))
}

#[cfg(windows)]
fn strip_verbatim(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(stripped) = s.strip_prefix(r"\\?\") {
        PathBuf::from(stripped)
    } else {
        path
    }
}

#[cfg(not(windows))]
fn strip_verbatim(path: PathBuf) -> PathBuf {
    path
}

pub fn relative_or_portable_absolute(path: &Path, root: Option<&Path>) -> PathBuf {
    if let Some(root) = root {
        if let Ok(relative) = path.strip_prefix(root) {
            return relative.to_path_buf();
        }
    }
    normalize_absolute_path(path)
}

pub fn normalize_absolute_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => push_prefix(&mut normalized, prefix.kind()),
            Component::RootDir => {}
            Component::CurDir => {}
            Component::ParentDir => normalized.push(".."),
            Component::Normal(segment) => normalized.push(segment),
        }
    }
    normalized
}

fn push_prefix(out: &mut PathBuf, prefix: Prefix<'_>) {
    match prefix {
        Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
            out.push((letter as char).to_string());
        }
        Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => {
            out.push("UNC");
            out.push(server.to_string_lossy().to_string());
            out.push(share.to_string_lossy().to_string());
        }
        _ => {}
    }
}
