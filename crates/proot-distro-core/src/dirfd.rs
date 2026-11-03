//! Directory file descriptor management and hardened descriptor VFS.

use anyhow::{anyhow, Result};
use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};
use std::path::{Path, PathBuf};

/// Hardened directory file descriptor wrapper with automated cleanup on drop.
pub struct DirFd {
    fd: RawFd,
    path: Option<PathBuf>,
}

impl DirFd {
    /// Open a directory file descriptor.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let p = path.as_ref();
        let c_path = CString::new(p.as_os_str().as_bytes())?;
        let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC;
        let fd = unsafe { libc::open(c_path.as_ptr(), flags) };

        if fd < 0 {
            return Err(anyhow!(
                "Failed to open directory '{}': {}",
                p.display(),
                std::io::Error::last_os_error()
            ));
        }

        Ok(Self {
            fd,
            path: Some(p.to_path_buf()),
        })
    }

    /// Wrap an existing raw file descriptor.
    pub fn from_raw_fd(fd: RawFd) -> Self {
        Self { fd, path: None }
    }

    /// Get raw file descriptor.
    pub fn as_raw_fd(&self) -> RawFd {
        self.fd
    }

    /// Consume wrapper and return raw fd without closing.
    pub fn into_raw_fd(mut self) -> RawFd {
        let fd = self.fd;
        self.fd = -1;
        fd
    }

    /// Associated path if available.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Open a subdirectory relative to this directory descriptor with O_NOFOLLOW | O_DIRECTORY | O_CLOEXEC.
    pub fn open_dir_at(&self, name: &str) -> Result<DirFd> {
        let c_name = CString::new(name)?;
        let flags = libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_DIRECTORY | libc::O_CLOEXEC;
        let sub_fd = unsafe { libc::openat(self.fd, c_name.as_ptr(), flags) };
        if sub_fd < 0 {
            return Err(anyhow!(
                "open_dir_at failed for '{}': {}",
                name,
                std::io::Error::last_os_error()
            ));
        }
        let sub_path = self.path.as_ref().map(|p| p.join(name));
        Ok(DirFd {
            fd: sub_fd,
            path: sub_path,
        })
    }

    /// Open a regular file relative to this directory descriptor.
    ///
    /// Uses O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC, then verifies via fstat that the file
    /// is S_IFREG, rejecting symlinks and FIFOs to prevent blocking attacks.
    pub fn open_regular_at(&self, name: &str, flags: i32) -> Result<std::fs::File> {
        let c_name = CString::new(name)?;
        let combined_flags = flags | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC;
        let fd = unsafe {
            if (combined_flags & libc::O_CREAT) != 0 {
                libc::openat(self.fd, c_name.as_ptr(), combined_flags, 0o644)
            } else {
                libc::openat(self.fd, c_name.as_ptr(), combined_flags)
            }
        };

        if fd < 0 {
            return Err(anyhow!(
                "open_regular_at failed for '{}': {}",
                name,
                std::io::Error::last_os_error()
            ));
        }

        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        if unsafe { libc::fstat(fd, &mut st) } < 0 {
            let err = std::io::Error::last_os_error();
            unsafe {
                libc::close(fd);
            }
            return Err(anyhow!("fstat failed on '{}': {}", name, err));
        }

        if (st.st_mode & libc::S_IFMT) != libc::S_IFREG {
            unsafe {
                libc::close(fd);
            }
            return Err(anyhow!(
                "rejected non-regular file '{}' (mode 0o{:o})",
                name,
                st.st_mode
            ));
        }

        // Clear O_NONBLOCK if caller did not request non-blocking I/O
        if (flags & libc::O_NONBLOCK) == 0 {
            unsafe {
                let cur = libc::fcntl(fd, libc::F_GETFL);
                if cur >= 0 {
                    libc::fcntl(fd, libc::F_SETFL, cur & !libc::O_NONBLOCK);
                }
            }
        }

        Ok(unsafe { std::fs::File::from_raw_fd(fd) })
    }

    /// Safely modify permissions on a verified descriptor.
    pub fn safe_chmod_at(&self, fd: RawFd, mode: u32) -> Result<()> {
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        if unsafe { libc::fstat(fd, &mut st) } < 0 {
            return Err(anyhow!(
                "safe_chmod_at fstat failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        let kind = st.st_mode & libc::S_IFMT;
        if kind != libc::S_IFREG && kind != libc::S_IFDIR {
            return Err(anyhow!(
                "safe_chmod_at: target is not regular file or directory (mode 0o{:o})",
                st.st_mode
            ));
        }
        if unsafe { libc::fchmod(fd, mode as libc::mode_t) } < 0 {
            return Err(anyhow!(
                "safe_chmod_at fchmod failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }

    /// Atomically write content using an exclusive temporary file followed by renameat.
    pub fn atomic_write_at(&self, name: &str, content: &[u8]) -> Result<()> {
        use std::io::Write;
        let tmp_name = format!(".tmp.{}.{}", name, uuid::Uuid::new_v4().simple());
        let c_tmp = CString::new(tmp_name.as_str())?;
        let c_dst = CString::new(name)?;

        let flags = libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC;
        let fd = unsafe { libc::openat(self.fd, c_tmp.as_ptr(), flags, 0o644) };
        if fd < 0 {
            return Err(anyhow!(
                "atomic_write_at failed to create temp file '{}': {}",
                tmp_name,
                std::io::Error::last_os_error()
            ));
        }

        let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
        if let Err(e) = file.write_all(content) {
            let _ = unsafe { libc::unlinkat(self.fd, c_tmp.as_ptr(), 0) };
            return Err(anyhow!("Failed to write to '{}': {}", tmp_name, e));
        }
        if let Err(e) = file.flush() {
            let _ = unsafe { libc::unlinkat(self.fd, c_tmp.as_ptr(), 0) };
            return Err(anyhow!("Failed to flush to '{}': {}", tmp_name, e));
        }
        drop(file);

        let ret = unsafe { libc::renameat(self.fd, c_tmp.as_ptr(), self.fd, c_dst.as_ptr()) };
        if ret < 0 {
            let err = std::io::Error::last_os_error();
            let _ = unsafe { libc::unlinkat(self.fd, c_tmp.as_ptr(), 0) };
            return Err(anyhow!(
                "atomic_write_at rename '{}' -> '{}' failed: {}",
                tmp_name,
                name,
                err
            ));
        }

        Ok(())
    }

    pub fn read_dir(&self) -> Result<Vec<std::fs::DirEntry>> {
        let path = self.path.as_ref().ok_or_else(|| anyhow!("Path not set"))?;
        let mut entries = Vec::new();
        let mut dir = std::fs::read_dir(path)?;
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
            return Err(anyhow!(
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
            return Err(anyhow!(
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
            return Err(anyhow!(
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
            return Err(anyhow!(
                "Failed to rename: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }
}

impl AsRawFd for DirFd {
    fn as_raw_fd(&self) -> RawFd {
        self.fd
    }
}

impl Drop for DirFd {
    fn drop(&mut self) {
        if self.fd >= 0 {
            unsafe {
                libc::close(self.fd);
            }
            self.fd = -1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn dirfd_atomic_write_and_open_regular() {
        let tmp = std::env::temp_dir().join(format!("pdn-dirfd-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let dir = DirFd::open(&tmp).expect("open dir");
        dir.atomic_write_at("hello.txt", b"world")
            .expect("atomic_write");

        let mut f = dir
            .open_regular_at("hello.txt", libc::O_RDONLY)
            .expect("open_regular");
        let mut buf = String::new();
        f.read_to_string(&mut buf).unwrap();
        assert_eq!(buf, "world");

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
