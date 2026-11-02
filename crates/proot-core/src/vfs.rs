//! Virtual File System (VFS) manager for PRoot

use anyhow::Result;
use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// VFS Manager for PRoot
pub struct VfsManager {
    rootfs: PathBuf,
    bind_mounts: Vec<crate::cli::BindMount>,
    mount_points: Arc<Mutex<HashMap<PathBuf, MountPoint>>>,
    fd_counter: Arc<Mutex<u64>>,
}

#[derive(Debug, Clone)]
pub struct MountPoint {
    pub source: PathBuf,
    pub target: PathBuf,
    pub fstype: String,
    pub flags: u64,
    pub readonly: bool,
}

impl VfsManager {
    pub fn new(config: &crate::cli::ProotConfig) -> Result<Self> {
        let rootfs = config.rootfs.clone().unwrap_or_else(|| PathBuf::from("/"));

        let bind_mounts = config.parse_bind_mounts();

        Ok(Self {
            rootfs,
            bind_mounts,
            mount_points: Arc::new(Mutex::new(HashMap::new())),
            fd_counter: Arc::new(Mutex::new(3)), // 0,1,2 reserved for stdin, stdout, stderr
        })
    }

    pub fn initialize(&self) -> Result<()> {
        // Create essential directories
        let dirs = [
            "proc",
            "sys",
            "dev",
            "run",
            "tmp",
            "run/dbus",
            "sys/fs/cgroup",
            "sys/class",
            "dev/shm",
        ];

        for dir in &dirs {
            let path = self.rootfs.join(dir);
            std::fs::create_dir_all(path)?;
        }

        // Create /run/dbus/system_bus_socket stub
        let socket_path = self.rootfs.join("run/dbus/system_bus_socket");
        if !socket_path.exists() {
            std::fs::File::create(&socket_path)?;
        }

        // Set permissions
        let mut perms = fs::metadata(self.rootfs.join("tmp"))?.permissions();
        perms.set_mode(0o1777);
        let _ = std::fs::set_permissions(self.rootfs.join("tmp"), perms);

        let mut perms = fs::metadata(self.rootfs.join("dev/shm"))?.permissions();
        perms.set_mode(0o1777);
        let _ = std::fs::set_permissions(self.rootfs.join("dev/shm"), perms);

        Ok(())
    }

    pub fn translate_path(&self, guest_path: &Path) -> PathBuf {
        // Translate guest path to host path
        let guest_path = Path::new(guest_path);
        if guest_path.is_absolute() {
            // Strip guest root prefix
            let relative = guest_path.strip_prefix("/").unwrap_or(guest_path);
            PathBuf::from("/").join(relative)
        } else {
            PathBuf::from("/").join(guest_path)
        }
    }

    pub fn resolve_path(&self, tracee: &crate::tracee::Tracee, path: &Path) -> PathBuf {
        // Resolve path relative to tracee's cwd
        if path.is_absolute() {
            self.translate_path(path)
        } else {
            let cwd = PathBuf::from(&tracee.path_info.cwd);
            self.translate_path(&cwd.join(path))
        }
    }

    pub fn check_access(&self, tracee: &crate::tracee::Tracee, path: &Path, _mode: u32) -> bool {
        let _host_path = self.resolve_path(tracee, path);
        // Check permissions
        // In a real implementation, this would check actual permissions
        true
    }

    pub fn get_next_fd(&self) -> u64 {
        let mut counter = self.fd_counter.lock().unwrap();
        let fd = *counter;
        *counter += 1;
        fd
    }

    pub fn get_rootfs(&self) -> &PathBuf {
        &self.rootfs
    }

    pub fn get_bind_mounts(&self) -> &[crate::cli::BindMount] {
        &self.bind_mounts
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_vfs_manager_creation() {
        let config = crate::cli::ProotConfig::default();
        let vfs = VfsManager::new(&config);
        assert!(vfs.is_ok());
    }
}
