//! Path management for PRoot Distro

use anyhow::Result;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Resolve a writable temp base directory from explicit environment values.
///
/// Split out from [`temp_base`] so the resolution order is testable without
/// mutating the process environment.
fn resolve_temp_base(tmpdir: Option<OsString>, prefix: Option<OsString>) -> PathBuf {
    tmpdir
        .map(PathBuf::from)
        .or_else(|| prefix.map(|p| PathBuf::from(p).join("tmp")))
        .unwrap_or_else(std::env::temp_dir)
}

/// Resolve a writable temp base directory.
///
/// `/tmp` is not writable by an app uid on Android, and `std::env::temp_dir()`
/// falls back to it when `TMPDIR` is unset, so it must never be used alone.
/// Order: `$TMPDIR` -> `$PREFIX/tmp` -> `std::env::temp_dir()`.
pub fn temp_base() -> PathBuf {
    resolve_temp_base(std::env::var_os("TMPDIR"), std::env::var_os("PREFIX"))
}

/// Container paths
pub struct ContainerPaths {
    root_dir: PathBuf,
    pub containers_dir: PathBuf,
    pub images_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub log_dir: PathBuf,
    pub config_dir: PathBuf,
}

impl ContainerPaths {
    pub fn new() -> Result<Self> {
        let root = dirs::home_dir()
            .or_else(|| std::env::var("HOME").ok().map(PathBuf::from))
            .or_else(|| std::env::var("USERPROFILE").ok().map(PathBuf::from))
            .or_else(|| std::env::var("PN_TEST_ROOT").ok().map(PathBuf::from))
            .unwrap_or_else(|| temp_base().join("pn-next-test"));

        // Ensure we can create the directory
        std::fs::create_dir_all(&root)?;

        // Use .podnext directory for consistency with native CLI
        let podnext_root = root.join(".podnext");
        std::fs::create_dir_all(&podnext_root)?;

        Ok(Self {
            root_dir: root.clone(),
            containers_dir: podnext_root.join("containers"),
            images_dir: podnext_root.join("images"),
            cache_dir: podnext_root.join("cache"),
            log_dir: podnext_root.join("logs"),
            config_dir: podnext_root.join("config"),
        })
    }

    pub fn container_rootfs(&self, name: &str) -> PathBuf {
        self.containers_dir.join(name).join("rootfs")
    }

    pub fn container_config(&self, name: &str) -> PathBuf {
        self.containers_dir.join(name).join("config.json")
    }

    pub fn container_state(&self, name: &str) -> PathBuf {
        self.containers_dir.join(name).join("state.json")
    }

    pub fn container_logs(&self, name: &str) -> PathBuf {
        self.log_dir.join(name)
    }

    pub fn image_path(&self, name: &str) -> PathBuf {
        self.images_dir.join(name)
    }

    pub fn ensure_dirs(&self) -> Result<()> {
        for dir in &[
            &self.containers_dir,
            &self.images_dir,
            &self.cache_dir,
            &self.log_dir,
            &self.config_dir,
        ] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}

/// Path resolver for guest/host path translation
pub struct PathResolver {
    guest_root: PathBuf,
    host_root: PathBuf,
}

impl PathResolver {
    pub fn new(guest_root: PathBuf, host_root: PathBuf) -> Self {
        Self {
            guest_root,
            host_root,
        }
    }

    pub fn guest_to_host(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            let relative = path.strip_prefix(&self.guest_root).unwrap_or(path);
            self.host_root.join(relative)
        } else {
            self.host_root.join(path)
        }
    }

    pub fn host_to_guest(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            let relative = path.strip_prefix(&self.host_root).unwrap_or(path);
            self.guest_root.join(relative)
        } else {
            self.guest_root.join(path)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tmpdir_takes_priority() {
        let got = resolve_temp_base(
            Some(OsString::from("/custom/tmp")),
            Some(OsString::from("/data/usr")),
        );
        assert_eq!(got, PathBuf::from("/custom/tmp"));
    }

    #[test]
    fn prefix_tmp_used_when_tmpdir_unset_and_is_not_tmp() {
        let got = resolve_temp_base(None, Some(OsString::from("/data/usr")));
        assert_eq!(got, PathBuf::from("/data/usr/tmp"));
        assert_ne!(got, PathBuf::from("/tmp"));
    }
}
