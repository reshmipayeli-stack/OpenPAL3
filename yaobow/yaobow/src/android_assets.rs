//! First-run setup of the PAL3 game data on Android.
//!
//! Files shared into an Android container from the host usually arrive as
//! single files, and pickers often cap how many can be shared at once, so
//! the game data is shipped as one `PAL3.zip`. On startup, if the app's
//! own storage has no PAL3 data yet, look for that zip in shared storage
//! and extract it to `<app storage>/PAL3`.

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

const ZIP_NAME: &str = "PAL3.zip";

fn has_pal3_data(dir: &Path) -> bool {
    dir.join("basedata").join("basedata.cpk").exists()
}

/// Search shared storage for `PAL3.zip`, a few directory levels deep.
fn find_zip() -> Option<PathBuf> {
    let roots = [
        "/sdcard",
        "/storage/emulated/0",
        "/mnt/sdcard",
        "/storage",
        "/storage/media/100/local/files/Docs",
    ];
    for root in roots {
        if let Some(p) = search(Path::new(root), 0) {
            return Some(p);
        }
    }
    None
}

fn search(dir: &Path, depth: usize) -> Option<PathBuf> {
    if depth > 3 {
        return None;
    }
    let entries = fs::read_dir(dir).ok()?;
    let mut subdirs = vec![];
    for e in entries.flatten() {
        let path = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if name.eq_ignore_ascii_case(ZIP_NAME) && path.is_file() {
            return Some(path);
        }
        // Skip app-private and very large system trees.
        if path.is_dir() && name != "Android" && !name.starts_with('.') {
            subdirs.push(path);
        }
    }
    subdirs.into_iter().find_map(|d| search(&d, depth + 1))
}

fn extract(zip_path: &Path, dest: &Path) -> anyhow::Result<()> {
    let tmp = dest.with_extension("extracting");
    if tmp.exists() {
        fs::remove_dir_all(&tmp)?;
    }
    fs::create_dir_all(&tmp)?;

    let mut archive = zip::ZipArchive::new(File::open(zip_path)?)?;
    let total = archive.len();
    log::info!("extracting {} entries from {}", total, zip_path.display());
    for i in 0..total {
        let mut entry = archive.by_index(i)?;
        let Some(rel) = entry.enclosed_name() else {
            log::warn!("skipping unsafe zip entry {}", entry.name());
            continue;
        };
        let out = tmp.join(rel);
        if entry.is_dir() {
            fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent)?;
        }
        io::copy(&mut entry, &mut File::create(&out)?)?;
        if i % 25 == 0 || i + 1 == total {
            log::info!("extracted {}/{}", i + 1, total);
        }
    }

    if dest.exists() {
        fs::remove_dir_all(dest)?;
    }
    fs::rename(&tmp, dest)?;
    Ok(())
}

/// Make sure `<app_dir>/PAL3` holds the game data, extracting `PAL3.zip`
/// from shared storage if needed. Errors are logged, not fatal: the asset
/// path resolver still looks in the other known locations.
pub fn ensure_pal3_data(app_dir: &Path) {
    let dest = app_dir.join("PAL3");
    if has_pal3_data(&dest) {
        log::info!("PAL3 data present at {}", dest.display());
        return;
    }
    match find_zip() {
        Some(zip_path) => {
            log::info!("found {}", zip_path.display());
            match extract(&zip_path, &dest) {
                Ok(()) if has_pal3_data(&dest) => {
                    log::info!("PAL3 data extracted to {}", dest.display())
                }
                Ok(()) => log::error!(
                    "{} extracted but has no basedata/basedata.cpk",
                    zip_path.display()
                ),
                Err(e) => log::error!("failed to extract {}: {e:#}", zip_path.display()),
            }
        }
        None => log::warn!("no PAL3 data in app storage and no {ZIP_NAME} found"),
    }
}
