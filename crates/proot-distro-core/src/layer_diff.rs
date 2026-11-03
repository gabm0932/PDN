//! OCI layer differ for PDN container image builder

use anyhow::{bail, Context, Result};
use flate2::write::GzEncoder;
use flate2::Compression;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Writer that transparently computes SHA-256 and tracks total uncompressed bytes written
struct CountingHasher<W: Write> {
    inner: W,
    hasher: Sha256,
    bytes_written: u64,
}

impl<W: Write> CountingHasher<W> {
    fn new(inner: W) -> Self {
        Self {
            inner,
            hasher: Sha256::new(),
            bytes_written: 0,
        }
    }

    fn finish(self) -> (W, String, u64) {
        let digest = format!("sha256:{}", hex::encode(self.hasher.finalize()));
        (self.inner, digest, self.bytes_written)
    }
}

impl<W: Write> Write for CountingHasher<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.hasher.update(&buf[..n]);
        self.bytes_written += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// Recursively collect all relative paths within a root directory in lexicographical order
fn collect_relative_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let mut relative_paths = Vec::new();
    if !root.exists() {
        return Ok(relative_paths);
    }

    let mut stack = vec![PathBuf::new()];
    while let Some(rel) = stack.pop() {
        let full = root.join(&rel);
        if !full.is_dir() {
            continue;
        }

        let mut entries = Vec::new();
        for entry in fs::read_dir(&full)? {
            entries.push(entry?);
        }
        // Deterministic sort within each directory level
        entries.sort_by_key(|e| e.file_name());

        for entry in entries {
            let child_rel = rel.join(entry.file_name());
            let ft = entry.file_type()?;
            if ft.is_dir() {
                stack.push(child_rel.clone());
            }
            relative_paths.push(child_rel);
        }
    }

    relative_paths.sort();
    Ok(relative_paths)
}

/// Compute SHA-256 digest of a regular file
fn compute_file_sha256(path: &Path) -> Result<[u8; 32]> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hasher.finalize().into())
}

/// Check if a regular file was modified between base and modified rootfs
fn is_file_modified(
    base_path: &Path,
    mod_path: &Path,
    base_meta: &fs::Metadata,
    mod_meta: &fs::Metadata,
) -> Result<bool> {
    if base_meta.len() != mod_meta.len() {
        return Ok(true);
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if (base_meta.mode() & 0o7777) != (mod_meta.mode() & 0o7777) {
            return Ok(true);
        }
    }

    let base_hash = compute_file_sha256(base_path)?;
    let mod_hash = compute_file_sha256(mod_path)?;
    Ok(base_hash != mod_hash)
}

/// Computes the diff between two rootfs directories and outputs an OCI-compliant tar.gz layer.
///
/// - Added and modified files are included directly in the tar archive.
/// - Deleted files and directories are converted into OCI whiteout markers (`.wh.<filename>`).
/// - Returns `(sha256_digest, uncompressed_size)` where `sha256_digest` is the OCI diffID
///   (the SHA-256 digest of the uncompressed tar stream).
pub fn compute_layer_diff(
    base_rootfs: &Path,
    modified_rootfs: &Path,
    output_tar: &Path,
) -> Result<(String, u64)> {
    if !modified_rootfs.exists() {
        bail!(
            "Modified rootfs directory does not exist: {}",
            modified_rootfs.display()
        );
    }

    if let Some(parent) = output_tar.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create parent directory {}", parent.display()))?;
    }

    let base_paths = collect_relative_paths(base_rootfs)?;
    let mod_paths = collect_relative_paths(modified_rootfs)?;

    // Step 1: Detect deleted entries in base_rootfs and generate OCI whiteout paths
    let mut deleted_whiteouts = Vec::new();
    let mut deleted_dirs = HashSet::new();

    for rel in &base_paths {
        // Skip paths under an already whiteouted directory
        let mut under_deleted_dir = false;
        let mut current = rel.parent();
        while let Some(p) = current {
            if p.as_os_str().is_empty() {
                break;
            }
            if deleted_dirs.contains(p) {
                under_deleted_dir = true;
                break;
            }
            current = p.parent();
        }
        if under_deleted_dir {
            continue;
        }

        let mod_full = modified_rootfs.join(rel);
        if fs::symlink_metadata(&mod_full).is_err() {
            let file_name = match rel.file_name() {
                Some(name) => name.to_string_lossy().to_string(),
                None => continue,
            };
            let wh_name = format!(".wh.{}", file_name);
            let wh_path = match rel.parent() {
                Some(parent) if !parent.as_os_str().is_empty() => parent.join(wh_name),
                _ => PathBuf::from(wh_name),
            };
            deleted_whiteouts.push(wh_path);

            let base_full = base_rootfs.join(rel);
            if let Ok(meta) = fs::symlink_metadata(&base_full) {
                if meta.is_dir() {
                    deleted_dirs.insert(rel.clone());
                }
            }
        }
    }

    // Step 2: Detect added or modified files in modified_rootfs
    let mut added_or_modified = Vec::new();

    for rel in &mod_paths {
        let mod_full = modified_rootfs.join(rel);
        let mod_meta = fs::symlink_metadata(&mod_full)
            .with_context(|| format!("Failed to stat {}", mod_full.display()))?;

        let base_full = base_rootfs.join(rel);
        let base_meta_res = fs::symlink_metadata(&base_full);

        match base_meta_res {
            Err(_) => {
                // Not in base_rootfs -> ADDED
                added_or_modified.push(rel.clone());
            }
            Ok(base_meta) => {
                // In base_rootfs -> check if modified
                if base_meta.file_type() != mod_meta.file_type() {
                    added_or_modified.push(rel.clone());
                } else if mod_meta.is_dir() {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::MetadataExt;
                        if (base_meta.mode() & 0o7777) != (mod_meta.mode() & 0o7777) {
                            added_or_modified.push(rel.clone());
                        }
                    }
                } else if mod_meta.is_symlink() {
                    let base_target = fs::read_link(&base_full)?;
                    let mod_target = fs::read_link(&mod_full)?;
                    if base_target != mod_target {
                        added_or_modified.push(rel.clone());
                    }
                } else if mod_meta.is_file() {
                    if is_file_modified(&base_full, &mod_full, &base_meta, &mod_meta)? {
                        added_or_modified.push(rel.clone());
                    }
                }
            }
        }
    }

    // Step 3: Archive diff entries into output tar.gz while hashing uncompressed tar
    let out_file = File::create(output_tar)
        .with_context(|| format!("Failed to create output archive {}", output_tar.display()))?;
    let gz_encoder = GzEncoder::new(out_file, Compression::default());
    let counting_hasher = CountingHasher::new(gz_encoder);
    let mut archive = tar::Builder::new(counting_hasher);

    // Append whiteout files first
    deleted_whiteouts.sort();
    for wh_path in &deleted_whiteouts {
        let mut header = tar::Header::new_gnu();
        header.set_path(wh_path)?;
        header.set_size(0);
        header.set_mode(0o644);
        header.set_entry_type(tar::EntryType::Regular);
        header.set_cksum();
        archive.append_data(&mut header, wh_path, std::io::empty())?;
    }

    // Append added and modified files
    added_or_modified.sort();
    for rel in &added_or_modified {
        let mod_full = modified_rootfs.join(rel);
        archive.append_path_with_name(&mod_full, rel)?;
    }

    archive.finish()?;
    let counting_hasher = archive.into_inner()?;
    let (gz_encoder, sha256_digest, uncompressed_size) = counting_hasher.finish();
    let mut out_file = gz_encoder.finish()?;
    out_file.flush()?;

    Ok((sha256_digest, uncompressed_size))
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::GzDecoder;
    use tar::Archive;

    #[test]
    fn test_compute_layer_diff() {
        let temp_dir = tempfile::tempdir().unwrap();
        let base = temp_dir.path().join("base");
        let modified = temp_dir.path().join("modified");
        let output_tar = temp_dir.path().join("diff.tar.gz");

        fs::create_dir_all(base.join("etc")).unwrap();
        fs::create_dir_all(base.join("deleted_dir")).unwrap();
        fs::create_dir_all(modified.join("etc")).unwrap();
        fs::create_dir_all(modified.join("new_dir")).unwrap();

        // 1. Unchanged file
        fs::write(base.join("etc/hosts"), "127.0.0.1 localhost\n").unwrap();
        fs::write(modified.join("etc/hosts"), "127.0.0.1 localhost\n").unwrap();

        // 2. Modified file
        fs::write(base.join("etc/resolv.conf"), "nameserver 1.1.1.1\n").unwrap();
        fs::write(modified.join("etc/resolv.conf"), "nameserver 8.8.8.8\n").unwrap();

        // 3. Deleted file
        fs::write(base.join("etc/old.conf"), "deprecated\n").unwrap();

        // 4. Added file
        fs::write(modified.join("new_dir/app.txt"), "hello world\n").unwrap();

        // 5. Deleted dir
        fs::write(base.join("deleted_dir/file.txt"), "old file\n").unwrap();

        let (digest, uncompressed_size) =
            compute_layer_diff(&base, &modified, &output_tar).unwrap();

        assert!(digest.starts_with("sha256:"));
        assert!(uncompressed_size > 0);
        assert!(output_tar.exists());

        // Verify tar archive entries
        let tar_file = File::open(&output_tar).unwrap();
        let gz_decoder = GzDecoder::new(tar_file);
        let mut archive = Archive::new(gz_decoder);

        let mut paths = Vec::new();
        for entry in archive.entries().unwrap() {
            let entry = entry.unwrap();
            paths.push(entry.path().unwrap().to_string_lossy().to_string());
        }

        assert!(paths.contains(&"etc/.wh.old.conf".to_string()));
        assert!(paths.contains(&".wh.deleted_dir".to_string()));
        assert!(paths.contains(&"etc/resolv.conf".to_string()));
        assert!(paths.contains(&"new_dir/app.txt".to_string()));
        assert!(!paths.contains(&"etc/hosts".to_string()));
    }
}
