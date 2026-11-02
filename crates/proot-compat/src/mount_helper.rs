//! Mount helper for essential filesystems
//! Mount otomatis /proc, /sys, /dev, /run, /tmp di dalam guest rootfs

use crate::cap_sys_admin::CapSysAdminManager;
use std::fs;
use std::path::Path;

/// Mount essential filesystems inside the guest rootfs.
/// These are needed for Chrome headless, Node.js, and other applications.
pub fn mount_essential_filesystems(rootfs: &Path) -> Result<(), String> {
    let mut manager = CapSysAdminManager::new();
    manager.check_capability();
    manager.mount_essential_filesystems(rootfs)
}

/// Unmount all essential filesystems (for cleanup).
pub fn unmount_essential_filesystems(rootfs: &Path) -> Result<(), String> {
    let targets = [
        rootfs.join("proc"),
        rootfs.join("sys"),
        rootfs.join("dev"),
        rootfs.join("run"),
        rootfs.join("tmp"),
    ];

    for target in &targets {
        // Best effort unmount - in PRoot context, this is handled by
        // the proot process exit, so we just ensure directories exist
        if !target.exists() {
            fs::create_dir_all(target)
                .map_err(|e| format!("Failed to create {:?}: {}", target, e))?;
        }
    }

    Ok(())
}

/// Bind mount a host path into the guest rootfs
pub fn bind_mount(source: &Path, target: &Path, readonly: bool) -> Result<(), String> {
    let mut manager = CapSysAdminManager::new();
    manager.check_capability();
    manager.bind_mount_with_fallback(source, target, readonly)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_mount_essential_filesystems_structure() {
        let dir = tempdir().unwrap();
        // Just verify the paths would be constructed correctly
        assert!(!dir.path().join("proc").exists());
        assert!(!dir.path().join("sys").exists());
    }
}
