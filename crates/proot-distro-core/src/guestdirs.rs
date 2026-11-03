//! Runtime directories that live beside a container's rootfs and are bound into
//! the guest.
//!
//! Currently one: the `/dev/shm` store. PDN passed `--bind=/dev`, which masks
//! whatever the rootfs has under `dev/`, so the guest had **no** `/dev/shm` at
//! all:
//!
//! ```text
//! $ podnext login deb-pdn -- touch /dev/shm/x
//! touch: cannot touch '/dev/shm/x': No such file or directory
//! ```
//!
//! proot-distro gives every container a real, writable `shm/` sibling and binds
//! it (`proot_distro/shm.py`), which is why the same command succeeds there.
//! Anything using POSIX shared memory needs it — Chromium in particular writes
//! its shared-memory segments there (`--disable-dev-shm-usage` is a workaround,
//! not a fix).
//!
//! The directory is mode 1777: world-writable with the sticky bit, which is what
//! `/dev/shm` is on a real system.

use anyhow::Result;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Mode of a real `/dev/shm`.
const SHM_MODE: u32 = 0o1777;

/// The store's path, a sibling of the rootfs so it is removed with the container.
pub fn shm_dir(container_dir: &Path) -> PathBuf {
    container_dir.join("shm")
}

/// Create the store if missing and make sure its mode is 1777.
pub fn ensure(container_dir: &Path) -> Result<PathBuf> {
    let dir = shm_dir(container_dir);
    fs::create_dir_all(&dir)?;
    // create_dir_all honours umask, so the mode is applied explicitly.
    fs::set_permissions(&dir, fs::Permissions::from_mode(SHM_MODE))?;
    Ok(dir)
}

/// `--bind=<src>:<dst>` for the guest's `/dev/shm`.
///
/// Appended after `--bind=/dev` so it wins over the masked directory.
pub fn bindings(container_dir: &Path) -> Vec<String> {
    let dir = shm_dir(container_dir);
    if !dir.is_dir() {
        return Vec::new();
    }
    vec![format!("{}:/dev/shm", dir.display())]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_creates_a_world_writable_sticky_directory_and_binds_it() {
        let dir = std::env::temp_dir().join(format!("pdn-guestdirs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let shm = ensure(&dir).expect("ensure");
        assert!(shm.is_dir());

        let mode = fs::metadata(&shm).unwrap().permissions().mode() & 0o7777;
        assert_eq!(mode, SHM_MODE, "expected 1777, got {:o}", mode);

        let b = bindings(&dir);
        assert_eq!(b.len(), 1);
        assert!(b[0].ends_with(":/dev/shm"), "got {:?}", b);

        // Idempotent, and a second call must not change the mode.
        ensure(&dir).unwrap();
        let mode2 = fs::metadata(&shm).unwrap().permissions().mode() & 0o7777;
        assert_eq!(mode2, SHM_MODE);

        // No directory yet -> nothing to bind.
        let empty = std::env::temp_dir().join(format!("pdn-guestdirs-e-{}", std::process::id()));
        let _ = fs::remove_dir_all(&empty);
        fs::create_dir_all(&empty).unwrap();
        assert!(bindings(&empty).is_empty());

        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&empty);
    }
}
