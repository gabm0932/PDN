//! Locking mechanism for PRoot Distro

use anyhow::Result;
use std::fs;
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Lock manager for containers
pub struct LockManager {
    locks: std::collections::HashMap<String, Arc<Mutex<()>>>,
}

impl LockManager {
    pub fn new() -> Result<Self> {
        Ok(Self {
            locks: std::collections::HashMap::new(),
        })
    }

    pub fn acquire(&mut self, name: &str) -> Result<()> {
        let lock = self
            .locks
            .entry(name.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())));

        // Try to acquire lock with timeout
        let guard = lock.try_lock();
        if guard.is_err() {
            // Wait with timeout
            let start = std::time::Instant::now();
            while start.elapsed() < std::time::Duration::from_secs(30) {
                if let Ok(_) = lock.try_lock() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }

        Ok(())
    }

    pub fn release(&mut self, name: &str) {
        self.locks.remove(name);
    }

    pub fn try_acquire(&mut self, name: &str, timeout: std::time::Duration) -> bool {
        let _lock = self
            .locks
            .entry(name.to_string())
            .or_insert_with(|| std::sync::Arc::new(std::sync::Mutex::new(())));

        let start = std::time::Instant::now();
        while start.elapsed() < timeout {
            if let Ok(_) = self.locks.get(name).unwrap().try_lock() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        false
    }
}

/// File-based lock for cross-process synchronization
pub struct FileLock {
    path: std::path::PathBuf,
    file: Option<std::fs::File>,
}

impl FileLock {
    pub fn new(path: &std::path::Path) -> Self {
        Self {
            path: path.to_path_buf(),
            file: None,
        }
    }

    pub fn lock(&mut self) -> Result<()> {
        let _file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .open(&self.path)?;

        // Use flock for locking
        unsafe {
            let result = libc::flock(
                self.file.as_ref().map(|f| f.as_raw_fd()).unwrap_or(0),
                libc::LOCK_EX | libc::LOCK_NB,
            );

            if result != 0 {
                return Err(anyhow::anyhow!("Failed to acquire lock"));
            }
        }

        self.file = Some(self.file.take().unwrap());
        Ok(())
    }

    pub fn unlock(&mut self) -> Result<()> {
        if let Some(file) = self.file.take() {
            unsafe {
                libc::flock(file.as_raw_fd(), libc::LOCK_UN);
            }
        }
        Ok(())
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = self.unlock();
    }
}

/// Container lock for exclusive access
pub struct ContainerLock {
    lock_file: PathBuf,
    file: Option<fs::File>,
    exclusive: bool,
}

impl ContainerLock {
    pub fn new(name: &str, exclusive: bool) -> Self {
        let lock_file = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("/home/user"))
            .join(".pn-next")
            .join("locks")
            .join(format!("{}.lock", name));

        Self {
            lock_file,
            file: None,
            exclusive,
        }
    }

    pub fn acquire(&mut self) -> Result<()> {
        use std::fs::OpenOptions;

        fs::create_dir_all(self.lock_file.parent().unwrap())?;

        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&self.lock_file)?;

        // Try to acquire lock.
        //
        // The descriptor to lock is `file`, just opened. This used to be
        // `self.file.as_ref().unwrap()`, but `self.file` is only assigned below
        // — so the first call panicked on `Option::unwrap()` and no container
        // operation could ever take a lock.
        let result = unsafe {
            if self.exclusive {
                libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB)
            } else {
                libc::flock(file.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB)
            }
        };

        if result != 0 {
            return Err(anyhow::anyhow!(
                "Failed to acquire lock: {} is held by another process",
                self.lock_file.display()
            ));
        }

        self.file = Some(file);
        Ok(())
    }

    pub fn release(&mut self) -> Result<()> {
        if let Some(file) = self.file.take() {
            unsafe {
                libc::flock(file.as_raw_fd(), libc::LOCK_UN);
            }
        }
        Ok(())
    }
}

impl Drop for ContainerLock {
    fn drop(&mut self) {
        let _ = self.release();
    }
}
