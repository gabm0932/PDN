//! Path translation for PRoot

use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Path translator for guest/host path translation
pub struct PathTranslator {
    guest_root: PathBuf,
    host_root: PathBuf,
    bind_mounts: Vec<crate::cli::BindMount>,
    symlink_cache: HashMap<PathBuf, PathBuf>,
}

impl PathTranslator {
    pub fn new(config: &crate::cli::ProotConfig) -> Result<Self> {
        let guest_root = config.rootfs.clone().unwrap_or_else(|| PathBuf::from("/"));
        let host_root = std::env::current_dir()?;

        Ok(Self {
            guest_root,
            host_root,
            bind_mounts: config.parse_bind_mounts(),
            symlink_cache: HashMap::new(),
        })
    }

    pub fn translate_guest_to_host(&self, guest_path: &Path) -> PathBuf {
        if guest_path.is_absolute() {
            let relative = guest_path
                .strip_prefix(&self.guest_root)
                .unwrap_or(guest_path);
            self.host_root.join(relative)
        } else {
            self.host_root.join(guest_path)
        }
    }

    pub fn translate_host_to_guest(&self, host_path: &Path) -> PathBuf {
        if host_path.is_absolute() {
            let relative = host_path.strip_prefix(&self.host_root).unwrap_or(host_path);
            self.guest_root.join(relative)
        } else {
            self.guest_root.join(host_path)
        }
    }

    pub fn translate_syscall_args(
        &self,
        _args: &mut [u64],
        _path_info: &crate::tracee::PathInfo,
    ) -> Result<()> {
        // Translate path arguments in syscalls
        Ok(())
    }

    pub fn translate_syscall_retval(
        &self,
        _info: &mut crate::tracee::SyscallInfo,
        _path_info: &crate::tracee::PathInfo,
    ) -> Result<()> {
        // Translate return values that are paths
        Ok(())
    }

    pub fn resolve_symlink_chain(&self, path: &std::path::Path) -> Result<std::path::PathBuf> {
        crate::compat::path_translation::resolve_symlink_chain(path).map_err(|e| anyhow::anyhow!(e))
    }
}
