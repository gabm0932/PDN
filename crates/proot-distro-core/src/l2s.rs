//! Link2Symlink Tree Rewriter for PRoot containers.
//!
//! Repairs intermediate hard-link symlinks when containers are renamed or relocated.

use anyhow::Result;
use std::path::Path;

struct SigBlocker {
    oldset: libc::sigset_t,
    blocked: bool,
}

impl SigBlocker {
    fn block() -> Self {
        unsafe {
            let mut oldset = std::mem::zeroed();
            let mut newset = std::mem::zeroed();
            libc::sigemptyset(&mut newset);
            libc::sigaddset(&mut newset, libc::SIGINT);
            libc::sigaddset(&mut newset, libc::SIGTERM);
            let ret = libc::pthread_sigmask(libc::SIG_BLOCK, &newset, &mut oldset);
            Self {
                oldset,
                blocked: ret == 0,
            }
        }
    }
}

impl Drop for SigBlocker {
    fn drop(&mut self) {
        if self.blocked {
            unsafe {
                libc::pthread_sigmask(libc::SIG_SETMASK, &self.oldset, std::ptr::null_mut());
            }
        }
    }
}

/// Recursively rewrite all link2symlink targets under `rootfs` from `old_rootfs` to `new_rootfs`.
///
/// Guarded with signal blockers (`SIGINT`/`SIGTERM`) to ensure atomic completion.
pub fn rewrite_l2s_tree(rootfs: &Path, old_rootfs: &Path, new_rootfs: &Path) -> Result<usize> {
    let _sig_guard = SigBlocker::block();
    let mut count = 0;
    walk(rootfs, old_rootfs, new_rootfs, &mut count)?;
    Ok(count)
}

fn walk(dir: &Path, old_rootfs: &Path, new_rootfs: &Path, count: &mut usize) -> Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    let old_canon = std::fs::canonicalize(old_rootfs).ok();
    let old_str = old_rootfs.to_string_lossy();
    let old_canon_str = old_canon.as_ref().map(|p| p.to_string_lossy());

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        let path = entry.path();

        if file_type.is_symlink() {
            if let Ok(target) = std::fs::read_link(&path) {
                let target_str = target.to_string_lossy();
                if target_str.contains(".proot.l2s.") || target_str.contains(".l2s.") {
                    let new_target = if let Ok(suffix) = target.strip_prefix(old_rootfs) {
                        Some(new_rootfs.join(suffix))
                    } else if let Some(canon) = &old_canon {
                        if let Ok(suffix) = target.strip_prefix(canon) {
                            Some(new_rootfs.join(suffix))
                        } else {
                            None
                        }
                    } else if target_str.starts_with(old_str.as_ref()) {
                        let remainder = &target_str[old_str.len()..];
                        let remainder = remainder.strip_prefix('/').unwrap_or(remainder);
                        Some(new_rootfs.join(remainder))
                    } else if let Some(canon_str) = &old_canon_str {
                        if target_str.starts_with(canon_str.as_ref()) {
                            let remainder = &target_str[canon_str.len()..];
                            let remainder = remainder.strip_prefix('/').unwrap_or(remainder);
                            Some(new_rootfs.join(remainder))
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    if let Some(new_target) = new_target {
                        if let Some(parent) = path.parent() {
                            let tmp_symlink = parent.join(format!(
                                ".tmp.l2s.{}.{}",
                                std::process::id(),
                                uuid::Uuid::new_v4().simple()
                            ));
                            #[cfg(unix)]
                            {
                                if std::os::unix::fs::symlink(&new_target, &tmp_symlink).is_ok() {
                                    if std::fs::rename(&tmp_symlink, &path).is_ok() {
                                        *count += 1;
                                    } else {
                                        let _ = std::fs::remove_file(&tmp_symlink);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        } else if file_type.is_dir() {
            walk(&path, old_rootfs, new_rootfs, count)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrite_l2s_symlinks() {
        let base = std::env::temp_dir().join(format!("pdn-l2s-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        let old_root = base.join("old_rootfs");
        let new_root = base.join("new_rootfs");
        let test_dir = base.join("container");
        std::fs::create_dir_all(&old_root).unwrap();
        std::fs::create_dir_all(&new_root).unwrap();
        std::fs::create_dir_all(&test_dir).unwrap();

        // Target with .proot.l2s.
        let l2s_target = old_root.join(".proot.l2s.1001");
        let l2s_link = test_dir.join("hardlinked_file");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&l2s_target, &l2s_link).unwrap();

        // Regular symlink that shouldn't be touched
        let regular_link = test_dir.join("regular_link");
        #[cfg(unix)]
        std::os::unix::fs::symlink(Path::new("/bin/sh"), &regular_link).unwrap();

        let rewritten = rewrite_l2s_tree(&test_dir, &old_root, &new_root).unwrap();
        assert_eq!(rewritten, 1);

        let target = std::fs::read_link(&l2s_link).unwrap();
        assert_eq!(target, new_root.join(".proot.l2s.1001"));

        let reg_target = std::fs::read_link(&regular_link).unwrap();
        assert_eq!(reg_target, Path::new("/bin/sh"));

        let _ = std::fs::remove_dir_all(&base);
    }
}
