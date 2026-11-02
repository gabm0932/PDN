//! Directory file descriptor management

use anyhow::Result;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::{FromRawFd, RawFd};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;

/// Directory file descriptor wrapper
pub struct DirFd {
    fd: RawFd,
    path: PathBuf,
    _guard: Arc<Mutex<()>>,
}

impl DirFd {
    pub fn open(path: &Path) -> Result<Self> {
        let fd = unsafe {
            libc::open(
                path.as_os_str().as_bytes().as_ptr() as *const libc::c_char,
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
            )
        };

        if fd < 0 {
            return Err(anyhow::anyhow!(
                "Failed to open directory: {}",
                std::io::Error::last_os_error()
            ));
        }

        Ok(Self {
            fd,
            path: path.to_path_buf(),
            _guard: Arc::new(Mutex::new(())),
        })
    }

    pub fn as_raw_fd(&self) -> RawFd {
        self.fd
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn read_dir(&self) -> Result<Vec<std::fs::DirEntry>> {
        let mut entries = Vec::new();
        let mut dir = std::fs::read_dir(&self.path)?;
        while let Some(entry) = dir.next() {
            entries.push(entry?);
        }
        Ok(entries)
    }

    pub fn open_file(&self, path: &Path, flags: i32, mode: u32) -> Result<std::fs::File> {
        let fd = unsafe {
            libc::openat(
                self.fd,
                path.as_os_str().as_bytes().as_ptr() as *const libc::c_char,
                flags,
                mode,
            )
        };

        if fd < 0 {
            return Err(anyhow::anyhow!(
                "Failed to open file: {}",
                std::io::Error::last_os_error()
            ));
        }

        Ok(unsafe { std::fs::File::from_raw_fd(fd) })
    }

    pub fn create_dir(&self, path: &Path, mode: u32) -> Result<()> {
        let result = unsafe {
            libc::mkdirat(
                self.fd,
                path.as_os_str().as_bytes().as_ptr() as *const libc::c_char,
                mode,
            )
        };

        if result < 0 {
            return Err(anyhow::anyhow!(
                "Failed to create directory: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }

    pub fn unlink(&self, path: &Path) -> Result<()> {
        let result = unsafe {
            libc::unlinkat(
                self.fd,
                path.as_os_str().as_bytes().as_ptr() as *const libc::c_char,
                0,
            )
        };

        if result < 0 {
            return Err(anyhow::anyhow!(
                "Failed to unlink: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }

    pub fn rename(&self, old: &Path, new: &Path) -> Result<()> {
        let result = unsafe {
            libc::renameat(
                self.fd,
                old.as_os_str().as_bytes().as_ptr() as *const libc::c_char,
                self.fd,
                new.as_os_str().as_bytes().as_ptr() as *const libc::c_char,
            )
        };

        if result < 0 {
            return Err(anyhow::anyhow!(
                "Failed to rename: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }
}

impl Drop for DirFd {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.fd);
        }
    }
}
