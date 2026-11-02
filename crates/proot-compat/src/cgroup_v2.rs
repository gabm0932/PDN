//! Cgroup v2 support for container isolation
//! Setup cgroup v2 mount dan konfigurasi di dalam guest rootfs

use crate::cap_sys_admin::CapSysAdminManager;
use std::path::Path;

/// Setup cgroup v2 inside the guest rootfs.
/// This enables resource control (CPU, memory, IO, PIDs) for containers.
pub fn setup_cgroup_v2(rootfs: &Path) -> Result<(), String> {
    let mut manager = CapSysAdminManager::new();
    manager.check_capability();
    manager.setup_cgroup_v2(rootfs)
}

/// Check if cgroup v2 is available in the rootfs.
pub fn cgroup_v2_available(rootfs: &Path) -> bool {
    rootfs.join("sys/fs/cgroup/cgroup.controllers").exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_cgroup_v2_available_false_when_missing() {
        let dir = tempdir().unwrap();
        assert!(!cgroup_v2_available(dir.path()));
    }

    #[test]
    #[ignore = "requires CAP_SYS_ADMIN for mount syscall"]
    fn test_setup_cgroup_v2_creates_dirs() {
        let dir = tempdir().unwrap();
        // This will fail on non-Linux or without mount privileges,
        // but we test that directory creation works
        let _ = setup_cgroup_v2(dir.path()); // Best effort
        assert!(dir.path().join("sys/fs/cgroup/system.slice").exists());
        assert!(dir.path().join("sys/fs/cgroup/user.slice").exists());
        assert!(dir.path().join("sys/fs/cgroup/machine.slice").exists());
    }
}
