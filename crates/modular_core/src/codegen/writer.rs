//! Filesystem writer + optional prettier invocation.
//!
//! Writes via temp files + atomic rename so half-written files never sit in
//! the generated/ tree. Skips writes when the rendered content is byte-equal
//! to the existing file, which keeps regenerations fast and produces cleaner
//! mtime semantics for editor reload.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct WrittenFile {
    pub path: PathBuf,
    pub changed: bool,
}

pub fn write_if_changed(path: &Path, content: &str) -> std::io::Result<WrittenFile> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Ok(existing) = fs::read_to_string(path) {
        if existing == content {
            return Ok(WrittenFile {
                path: path.to_path_buf(),
                changed: false,
            });
        }
    }
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension()
            .and_then(|s| s.to_str())
            .unwrap_or("tmp")
    ));
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(content.as_bytes())?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    Ok(WrittenFile {
        path: path.to_path_buf(),
        changed: true,
    })
}

/// Invoke `prettier --write` on a directory. Failure is logged but non-fatal:
/// emitted files are committable as-is, and a missing prettier in CI shouldn't
/// block codegen output.
pub fn run_prettier(target_dir: &Path, repo_root: &Path) -> Result<(), String> {
    let prettier_bin = repo_root.join("node_modules/.bin/prettier");
    if !prettier_bin.exists() {
        eprintln!(
            "warning: {} not found — skipping format step",
            prettier_bin.display()
        );
        return Ok(());
    }
    let output = Command::new(&prettier_bin)
        .arg("--write")
        .arg(target_dir)
        .current_dir(repo_root)
        .output()
        .map_err(|e| format!("prettier failed to launch: {e}"))?;
    if !output.status.success() {
        if !output.stderr.is_empty() {
            eprintln!("prettier: {}", String::from_utf8_lossy(&output.stderr));
        }
        return Err(format!("prettier exited with {}", output.status));
    }
    Ok(())
}
