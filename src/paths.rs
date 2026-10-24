//! HEYU (鹤羽输入法) - Install-layout aware data directory resolution
//! ===================================================================
//! * Running from the repo / a portable folder (no marker file): data lives in `<base>/data`
//!   exactly as before.
//! * Installed under `C:\Program Files\HEYU` (install.ps1 writes `heyu_installed.marker` next to
//!   the exe): Program Files is read-only for the user, so the working data directory becomes
//!   `%APPDATA%\HEYU\data`. User files (config.json, user_custom.tsv, telemetry.json) are seeded
//!   once and never overwritten; system tables (*.tsv dictionaries, root_atlas.json, ...) are
//!   refreshed whenever the installed copy differs (e.g. after an update).

use std::fs;
use std::path::{Path, PathBuf};

pub const INSTALLED_MARKER: &str = "heyu_installed.marker";
pub const PORTABLE_FLAG: &str = "portable.flag";
/// Files owned by the user: copied only if missing, never overwritten.
pub const USER_FILES: [&str; 4] = ["config.json", "user_custom.tsv", "telemetry.json", "crash.log"];

/// `%APPDATA%\HEYU`
pub fn appdata_root() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|p| PathBuf::from(p).join("HEYU"))
}

pub fn is_portable_mode(base_dir: &Path) -> bool {
    base_dir.join(PORTABLE_FLAG).exists()
}

pub fn is_installed_layout(base_dir: &Path) -> bool {
    !is_portable_mode(base_dir) && base_dir.join(INSTALLED_MARKER).exists()
}

fn needs_refresh(src: &Path, dst: &Path) -> bool {
    let (s, d) = match (fs::metadata(src), fs::metadata(dst)) {
        (Ok(s), Ok(d)) => (s, d),
        (Ok(_), Err(_)) => return true,
        _ => return false,
    };
    if s.len() != d.len() {
        return true;
    }
    match (s.modified(), d.modified()) {
        (Ok(sm), Ok(dm)) => sm > dm,
        _ => false,
    }
}

/// Copy `<src_data>` into `<dst_data>` following the user/system file rules above.
/// Returns the number of files copied.
pub fn sync_data_dir(src_data: &Path, dst_data: &Path) -> std::io::Result<usize> {
    fs::create_dir_all(dst_data)?;
    let mut copied = 0;
    for entry in fs::read_dir(src_data)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = entry.file_name();
        let name_s = name.to_string_lossy().to_string();
        let dst = dst_data.join(&name);
        let is_user_file = USER_FILES.iter().any(|u| u.eq_ignore_ascii_case(&name_s));
        let copy = if is_user_file { !dst.exists() } else { needs_refresh(&path, &dst) };
        if copy && fs::copy(&path, &dst).is_ok() {
            copied += 1;
        }
    }
    Ok(copied)
}

/// Resolve the working data directory for the server.
/// `override_dir` (from `--data-dir`) wins; otherwise installed vs. portable layout.
pub fn resolve_data_dir(base_dir: &Path, override_dir: Option<&Path>) -> PathBuf {
    if let Some(d) = override_dir {
        return d.to_path_buf();
    }
    let portable = base_dir.join("data");
    if is_installed_layout(base_dir) {
        if let Some(root) = appdata_root() {
            let user_data = root.join("data");
            if sync_data_dir(&portable, &user_data).is_ok()
                && (user_data.join("heyu_sys.tsv").exists() || user_data.join("flypy_sys.tsv").exists())
            {
                return user_data;
            }
        }
    }
    portable
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_seeds_user_files_once_and_refreshes_system_files() {
        let tmp = std::env::temp_dir().join(format!("heyu_paths_test_{}", std::process::id()));
        let src = tmp.join("src");
        let dst = tmp.join("dst");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("config.json"), "{\"theme_index\": 1}").unwrap();
        fs::write(src.join("heyu_sys.tsv"), "a\tb\n").unwrap();
        assert_eq!(sync_data_dir(&src, &dst).unwrap(), 2);
        // user edits config, installer ships a new dictionary
        fs::write(dst.join("config.json"), "{\"theme_index\": 3}").unwrap();
        fs::write(src.join("heyu_sys.tsv"), "a\tb\nc\td\n").unwrap();
        sync_data_dir(&src, &dst).unwrap();
        assert_eq!(fs::read_to_string(dst.join("config.json")).unwrap(), "{\"theme_index\": 3}");
        assert_eq!(fs::read_to_string(dst.join("heyu_sys.tsv")).unwrap(), "a\tb\nc\td\n");
        // portable layout: no marker -> <base>/data
        assert_eq!(resolve_data_dir(&tmp, None), tmp.join("data"));
        assert_eq!(resolve_data_dir(&tmp, Some(&dst)), dst);
        let _ = fs::remove_dir_all(&tmp);
    }
}
