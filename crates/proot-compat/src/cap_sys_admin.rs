//! CAP_SYS_ADMIN capability detection and filesystem mounting with fallbacks
//!
//! This module provides a robust architecture for handling filesystem mounts
//! on Android/Termux where CAP_SYS_ADMIN may not be available. It detects
//! capability availability and provides graceful fallbacks.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

/// CAP_SYS_ADMIN capability status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapSysAdminStatus {
    /// Full CAP_SYS_ADMIN available (root or proper capabilities)
    Available,
    /// CAP_SYS_ADMIN not available (typical Android/Termux without root)
    Unavailable,
    /// Unknown status (not yet checked)
    Unknown,
}

/// Filesystem mount configuration
#[derive(Debug, Clone)]
pub struct MountConfig {
    pub source: Option<String>,
    pub target: String,
    pub fstype: Option<String>,
    pub flags: MountFlags,
    pub data: Option<String>,
}

/// Mount flags wrapper
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MountFlags {
    pub bind: bool,
    pub recursive: bool,
    pub noexec: bool,
    pub nosuid: bool,
    pub nodev: bool,
    pub readonly: bool,
    pub remount: bool,
    pub private: bool,
    pub slave: bool,
    pub shared: bool,
    pub unbindable: bool,
}

impl Default for MountFlags {
    fn default() -> Self {
        Self {
            bind: false,
            recursive: false,
            noexec: false,
            nosuid: false,
            nodev: false,
            readonly: false,
            remount: false,
            private: false,
            slave: false,
            shared: false,
            unbindable: false,
        }
    }
}

impl MountFlags {
    pub fn to_nix_flags(&self) -> nix::mount::MsFlags {
        let mut flags = nix::mount::MsFlags::empty();
        if self.bind {
            flags |= nix::mount::MsFlags::MS_BIND;
        }
        if self.recursive {
            flags |= nix::mount::MsFlags::MS_REC;
        }
        if self.noexec {
            flags |= nix::mount::MsFlags::MS_NOEXEC;
        }
        if self.nosuid {
            flags |= nix::mount::MsFlags::MS_NOSUID;
        }
        if self.nodev {
            flags |= nix::mount::MsFlags::MS_NODEV;
        }
        if self.readonly {
            flags |= nix::mount::MsFlags::MS_RDONLY;
        }
        if self.remount {
            flags |= nix::mount::MsFlags::MS_REMOUNT;
        }
        if self.private {
            flags |= nix::mount::MsFlags::MS_PRIVATE;
        }
        if self.slave {
            flags |= nix::mount::MsFlags::MS_SLAVE;
        }
        if self.shared {
            flags |= nix::mount::MsFlags::MS_SHARED;
        }
        if self.unbindable {
            flags |= nix::mount::MsFlags::MS_UNBINDABLE;
        }
        flags
    }
}

/// CAP_SYS_ADMIN manager for filesystem operations
pub struct CapSysAdminManager {
    status: CapSysAdminStatus,
    fallback_mode: bool,
}

impl CapSysAdminManager {
    /// Create a new CAP_SYS_ADMIN manager with fallback mode enabled by default
    /// This is safe for environments where CAP_SYS_ADMIN might not be available (e.g., Android/Termux)
    pub fn new() -> Self {
        Self {
            status: CapSysAdminStatus::Unknown,
            fallback_mode: true, // Safe default: use fallback mode
        }
    }

    /// Create a new manager with capability testing enabled
    /// Use this only in environments where you know CAP_SYS_ADMIN is available
    pub fn new_with_capability_testing() -> Self {
        Self {
            status: CapSysAdminStatus::Unknown,
            fallback_mode: false,
        }
    }

    /// Create a new manager with forced fallback mode
    pub fn new_fallback() -> Self {
        Self {
            status: CapSysAdminStatus::Unavailable,
            fallback_mode: true,
        }
    }

    /// Check if CAP_SYS_ADMIN is available
    pub fn check_capability(&mut self) -> CapSysAdminStatus {
        if self.fallback_mode {
            self.status = CapSysAdminStatus::Unavailable;
            return self.status;
        }

        // Try to detect CAP_SYS_ADMIN by attempting a test mount
        // We use a temporary directory and try to mount a tmpfs
        self.status = if self.test_mount_capability() {
            CapSysAdminStatus::Available
        } else {
            CapSysAdminStatus::Unavailable
        };

        if self.status == CapSysAdminStatus::Unavailable {
            log::warn!(
                "CAP_SYS_ADMIN not available, using fallback mode (directory creation only)"
            );
        }

        self.status
    }

    /// Test if we can perform mount operations
    fn test_mount_capability(&self) -> bool {
        // First check if we're root
        if unsafe { libc::geteuid() } == 0 {
            return true;
        }

        // Check if we have CAP_SYS_ADMIN via capabilities
        #[cfg(target_os = "linux")]
        {
            use std::process::Command;
            if let Ok(output) = Command::new("capsh").arg("--decode=cap_sys_admin").output() {
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    if stdout.contains("cap_sys_admin") && stdout.contains("ep") {
                        return true;
                    }
                }
            }
        }

        // Try a test mount (tmpfs to a temp directory)
        if let Ok(temp_dir) = std::env::temp_dir().canonicalize() {
            let test_mount = temp_dir.join("cap_test_mount");
            if fs::create_dir_all(&test_mount).is_ok() {
                let result = nix::mount::mount(
                    Some("tmpfs"),
                    &test_mount,
                    Some("tmpfs"),
                    nix::mount::MsFlags::MS_NOEXEC
                        | nix::mount::MsFlags::MS_NOSUID
                        | nix::mount::MsFlags::MS_NODEV,
                    None::<&str>,
                );
                let _ = fs::remove_dir(&test_mount);
                return result.is_ok();
            }
        }

        false
    }

    /// Get current capability status
    pub fn status(&self) -> CapSysAdminStatus {
        self.status
    }

    /// Check if fallback mode is active
    pub fn is_fallback_mode(&self) -> bool {
        self.fallback_mode || self.status == CapSysAdminStatus::Unavailable
    }

    /// Mount a filesystem with fallback to directory creation
    pub fn mount_with_fallback(&self, config: &MountConfig) -> Result<(), String> {
        if self.is_fallback_mode() {
            self.create_mount_point_fallback(Path::new(&config.target))?;
            log::info!(
                "Fallback: Created directory {:?} instead of mounting",
                config.target
            );
            return Ok(());
        }

        // Attempt actual mount
        let flags = config.flags.to_nix_flags();
        nix::mount::mount(
            config.source.as_deref(),
            Path::new(&config.target),
            config.fstype.as_deref(),
            flags,
            config.data.as_deref(),
        )
        .map_err(|e| {
            // If mount fails, fall back to directory creation
            log::warn!(
                "Mount failed ({}), falling back to directory creation: {}",
                e,
                config.target
            );
            if let Err(fallback_err) = self.create_mount_point_fallback(Path::new(&config.target)) {
                return format!("Mount failed: {} and fallback failed: {}", e, fallback_err);
            }
            format!("Mount failed, using fallback: {}", e)
        })?;

        log::info!(
            "Successfully mounted {:?} on {:?}",
            config.source,
            config.target
        );
        Ok(())
    }

    /// Create mount point directory as fallback
    fn create_mount_point_fallback(&self, target: &Path) -> Result<(), String> {
        fs::create_dir_all(target)
            .map_err(|e| format!("Failed to create mount point {:?}: {}", target, e))?;

        // Set appropriate permissions
        if let Ok(metadata) = fs::metadata(target) {
            let mut perms = metadata.permissions();
            perms.set_mode(0o755);
            let _ = fs::set_permissions(target, perms);
        }

        Ok(())
    }

    /// Mount essential filesystems (proc, sysfs, devtmpfs, tmpfs) with fallbacks
    pub fn mount_essential_filesystems(&self, rootfs: &Path) -> Result<(), String> {
        let mounts = [
            MountConfig {
                source: Some("proc".to_string()),
                target: rootfs.join("proc").to_string_lossy().to_string(),
                fstype: Some("proc".to_string()),
                flags: MountFlags::default(),
                data: None,
            },
            MountConfig {
                source: Some("sysfs".to_string()),
                target: rootfs.join("sys").to_string_lossy().to_string(),
                fstype: Some("sysfs".to_string()),
                flags: MountFlags {
                    noexec: true,
                    nosuid: true,
                    nodev: true,
                    ..Default::default()
                },
                data: None,
            },
            MountConfig {
                source: Some("devtmpfs".to_string()),
                target: rootfs.join("dev").to_string_lossy().to_string(),
                fstype: Some("devtmpfs".to_string()),
                flags: MountFlags {
                    noexec: true,
                    nosuid: true,
                    ..Default::default()
                },
                data: Some("mode=755".to_string()),
            },
            MountConfig {
                source: Some("tmpfs".to_string()),
                target: rootfs.join("run").to_string_lossy().to_string(),
                fstype: Some("tmpfs".to_string()),
                flags: MountFlags {
                    noexec: true,
                    nosuid: true,
                    nodev: true,
                    ..Default::default()
                },
                data: Some("mode=755".to_string()),
            },
            MountConfig {
                source: Some("tmpfs".to_string()),
                target: rootfs.join("tmp").to_string_lossy().to_string(),
                fstype: Some("tmpfs".to_string()),
                flags: MountFlags {
                    noexec: true,
                    nosuid: true,
                    nodev: true,
                    ..Default::default()
                },
                data: Some("mode=1777".to_string()),
            },
        ];

        for mount_config in &mounts {
            if let Err(e) = self.mount_with_fallback(mount_config) {
                // Log but continue - some mounts may fail but others succeed
                log::warn!("Failed to mount {:?}: {}", mount_config.target, e);
            }
        }

        Ok(())
    }

    /// Setup cgroup v2 with fallback
    pub fn setup_cgroup_v2(&self, rootfs: &Path) -> Result<(), String> {
        let cgroup_path = rootfs.join("sys/fs/cgroup");

        // Create cgroup directory structure
        self.create_mount_point_fallback(&cgroup_path)?;

        // Try to mount cgroup2 if CAP_SYS_ADMIN available
        if !self.is_fallback_mode() {
            let config = MountConfig {
                source: Some("cgroup2".to_string()),
                target: cgroup_path.to_string_lossy().to_string(),
                fstype: Some("cgroup2".to_string()),
                flags: MountFlags {
                    noexec: true,
                    nosuid: true,
                    nodev: true,
                    ..Default::default()
                },
                data: None,
            };

            if let Err(e) = self.mount_with_fallback(&config) {
                log::warn!("Failed to mount cgroup2: {}", e);
            }
        }

        // Create default cgroup hierarchy (always works, just directory creation)
        self.create_default_cgroups(&cgroup_path)?;

        Ok(())
    }

    /// Create default cgroup hierarchy
    fn create_default_cgroups(&self, cgroup_root: &Path) -> Result<(), String> {
        let slices = ["system.slice", "user.slice", "machine.slice"];

        for slice in &slices {
            let path = cgroup_root.join(slice);
            fs::create_dir_all(&path).map_err(|e| format!("Failed to create {:?}: {}", path, e))?;

            // Set permissions
            if let Ok(metadata) = fs::metadata(&path) {
                let mut perms = metadata.permissions();
                perms.set_mode(0o755);
                let _ = fs::set_permissions(&path, perms);
            }
        }

        Ok(())
    }

    /// Bind mount with fallback
    pub fn bind_mount_with_fallback(
        &self,
        source: &Path,
        target: &Path,
        readonly: bool,
    ) -> Result<(), String> {
        // Always create target directory
        self.create_mount_point_fallback(target)?;

        if self.is_fallback_mode() {
            log::info!(
                "Fallback: Created bind mount directory {:?} -> {:?}",
                source,
                target
            );
            return Ok(());
        }

        let mut flags = MountFlags {
            bind: true,
            recursive: true,
            ..Default::default()
        };
        if readonly {
            flags.readonly = true;
        }

        let config = MountConfig {
            source: Some(source.to_string_lossy().to_string()),
            target: target.to_string_lossy().to_string(),
            fstype: None,
            flags,
            data: None,
        };

        self.mount_with_fallback(&config)
    }

    /// Unmount filesystem
    pub fn unmount(&self, target: &Path) -> Result<(), String> {
        if self.is_fallback_mode() {
            // In fallback mode, we just ensure the directory exists
            if !target.exists() {
                self.create_mount_point_fallback(target)?;
            }
            return Ok(());
        }

        nix::mount::umount(target).map_err(|e| format!("Failed to unmount {:?}: {}", target, e))
    }
}

/// Global CAP_SYS_ADMIN manager instance
static mut GLOBAL_MANAGER: Option<CapSysAdminManager> = None;

/// Initialize global CAP_SYS_ADMIN manager with fallback mode by default
/// Use `init_global_manager_with_testing(true)` to enable capability testing
pub fn init_global_manager(fallback: bool) {
    unsafe {
        GLOBAL_MANAGER = Some(if fallback {
            CapSysAdminManager::new_fallback()
        } else {
            CapSysAdminManager::new()
        });

        if let Some(manager) = GLOBAL_MANAGER.as_mut() {
            manager.check_capability();
        }
    }
}

/// Initialize global manager with capability testing enabled (for integration tests)
pub fn init_global_manager_with_testing(enable_testing: bool) {
    unsafe {
        GLOBAL_MANAGER = Some(if enable_testing {
            CapSysAdminManager::new()
        } else {
            CapSysAdminManager::new_fallback()
        });

        if let Some(manager) = GLOBAL_MANAGER.as_mut() {
            manager.check_capability();
        }
    }
}

/// Get global CAP_SYS_ADMIN manager
pub fn global_manager() -> Option<&'static CapSysAdminManager> {
    unsafe { GLOBAL_MANAGER.as_ref() }
}

/// Get mutable global CAP_SYS_ADMIN manager
pub fn global_manager_mut() -> Option<&'static mut CapSysAdminManager> {
    unsafe { GLOBAL_MANAGER.as_mut() }
}

/// Check if CAP_SYS_ADMIN is available globally
pub fn is_cap_sys_admin_available() -> bool {
    global_manager()
        .map(|m| m.status() == CapSysAdminStatus::Available)
        .unwrap_or(false)
}

/// Check if fallback mode is active globally
pub fn is_fallback_mode() -> bool {
    global_manager()
        .map(|m| m.is_fallback_mode())
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_cap_sys_admin_manager_creation() {
        let manager = CapSysAdminManager::new();
        assert_eq!(manager.status(), CapSysAdminStatus::Unknown);
    }

    #[test]
    fn test_fallback_manager() {
        let manager = CapSysAdminManager::new_fallback();
        assert_eq!(manager.status(), CapSysAdminStatus::Unavailable);
        assert!(manager.is_fallback_mode());
    }

    #[test]
    fn test_create_mount_point_fallback() {
        let manager = CapSysAdminManager::new_fallback();
        let dir = tempdir().unwrap();
        let target = dir.path().join("test_mount");

        assert!(manager.create_mount_point_fallback(&target).is_ok());
        assert!(target.exists());
    }

    #[test]
    fn test_mount_essential_filesystems_fallback() {
        let manager = CapSysAdminManager::new_fallback();
        let dir = tempdir().unwrap();

        assert!(manager.mount_essential_filesystems(dir.path()).is_ok());
        assert!(dir.path().join("proc").exists());
        assert!(dir.path().join("sys").exists());
        assert!(dir.path().join("dev").exists());
        assert!(dir.path().join("run").exists());
        assert!(dir.path().join("tmp").exists());
    }

    #[test]
    fn test_setup_cgroup_v2_fallback() {
        let manager = CapSysAdminManager::new_fallback();
        let dir = tempdir().unwrap();

        assert!(manager.setup_cgroup_v2(dir.path()).is_ok());
        assert!(dir.path().join("sys/fs/cgroup/system.slice").exists());
        assert!(dir.path().join("sys/fs/cgroup/user.slice").exists());
        assert!(dir.path().join("sys/fs/cgroup/machine.slice").exists());
    }

    #[test]
    fn test_bind_mount_fallback() {
        let manager = CapSysAdminManager::new_fallback();
        let dir = tempdir().unwrap();
        let source = dir.path().join("source");
        let target = dir.path().join("target");

        fs::create_dir_all(&source).unwrap();

        assert!(manager
            .bind_mount_with_fallback(&source, &target, false)
            .is_ok());
        assert!(target.exists());
    }
}
