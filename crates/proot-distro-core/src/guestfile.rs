//! Guest file access for PRoot Distro

use anyhow::Result;
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Read bytes from guest filesystem
pub fn read_guest_bytes(
    rootfs: &Path,
    path: &Path,
    max_bytes: usize,
    _root_fd: Option<std::os::unix::io::RawFd>,
) -> Result<Vec<u8>> {
    let full_path = rootfs.join(path.strip_prefix("/").unwrap_or(path));

    let mut file = fs::File::open(&full_path)?;
    let mut buffer = vec![0u8; max_bytes];
    let bytes_read = file.read(&mut buffer)?;
    buffer.truncate(bytes_read);
    Ok(buffer)
}

/// Write bytes to guest filesystem
pub fn write_guest_bytes(
    rootfs: &Path,
    path: &Path,
    data: &[u8],
    _root_fd: Option<std::os::unix::io::RawFd>,
) -> Result<()> {
    let full_path = rootfs.join(path.strip_prefix("/").unwrap_or(path));

    if let Some(parent) = full_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut file = fs::File::create(&full_path)?;
    file.write_all(data)?;
    Ok(())
}

/// Read guest file as string
pub fn read_guest_string(
    rootfs: &Path,
    path: &Path,
    max_bytes: usize,
    root_fd: Option<std::os::unix::io::RawFd>,
) -> Result<String> {
    let bytes = read_guest_bytes(rootfs, path, max_bytes, root_fd)?;
    String::from_utf8(bytes).map_err(|e| anyhow::anyhow!("Invalid UTF-8: {}", e))
}

/// Guest file operations
pub struct GuestFile {
    path: PathBuf,
    rootfs: PathBuf,
    file: Option<fs::File>,
}

impl GuestFile {
    pub fn open(rootfs: &Path, path: &Path) -> Result<Self> {
        let full_path = rootfs.join(path.strip_prefix("/").unwrap_or(path));
        let file = fs::File::open(&full_path)?;
        Ok(Self {
            path: path.to_path_buf(),
            rootfs: rootfs.to_path_buf(),
            file: Some(file),
        })
    }

    pub fn create(rootfs: &Path, path: &Path) -> Result<Self> {
        let full_path = rootfs.join(path.strip_prefix("/").unwrap_or(path));
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = fs::File::create(&full_path)?;
        Ok(Self {
            path: path.to_path_buf(),
            rootfs: rootfs.to_path_buf(),
            file: Some(file),
        })
    }

    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        if let Some(file) = &mut self.file {
            file.read(buf).map_err(|e| anyhow::anyhow!(e))
        } else {
            Err(anyhow::anyhow!("File not open"))
        }
    }

    pub fn write(&mut self, buf: &[u8]) -> Result<usize> {
        if let Some(file) = &mut self.file {
            file.write(buf).map_err(|e| anyhow::anyhow!(e))
        } else {
            Err(anyhow::anyhow!("File not open"))
        }
    }

    pub fn seek(&mut self, pos: SeekFrom) -> Result<u64> {
        if let Some(file) = &mut self.file {
            file.seek(pos).map_err(|e| anyhow::anyhow!(e))
        } else {
            Err(anyhow::anyhow!("File not open"))
        }
    }

    pub fn close(&mut self) -> Result<()> {
        if let Some(file) = self.file.take() {
            drop(file);
        }
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}
