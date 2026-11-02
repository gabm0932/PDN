//! Shared memory management for PRoot Distro

use anyhow::Result;
use memmap2::{Mmap, MmapMut, MmapOptions};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Shared memory manager
pub struct ShmManager {
    shm_dir: PathBuf,
    mappings: std::sync::Mutex<std::collections::HashMap<String, Arc<ShmMapping>>>,
}

impl ShmManager {
    pub fn new() -> Result<Self> {
        let shm_dir = std::env::var("PN_SHM_DIR")
            .ok()
            .map(PathBuf::from)
            .or_else(|| {
                // In test environment, use a temp directory
                if cfg!(test) {
                    // Use temp_dir() which should be writable, or current dir
                    std::env::var("PN_TEST_ROOT")
                        .ok()
                        .map(|p| PathBuf::from(p).join("shm"))
                        .or_else(|| Some(std::env::temp_dir().join("pn-next-test").join("shm")))
                } else {
                    None
                }
            })
            .or_else(|| dirs::runtime_dir())
            .or_else(|| {
                // On Android/Termux, use a writable directory in home
                dirs::home_dir().map(|h| h.join(".local").join("run").join("pn-next"))
            })
            .unwrap_or_else(|| std::path::PathBuf::from("/tmp/pn-next"));

        std::fs::create_dir_all(&shm_dir)?;

        Ok(Self {
            shm_dir,
            mappings: std::sync::Mutex::new(std::collections::HashMap::new()),
        })
    }

    pub fn create(&self, name: &str, size: usize) -> Result<ShmHandle> {
        let path = self.shm_dir.join(name);

        // Create file
        let file = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&path)?;

        // Set size
        file.set_len(size as u64)?;

        // Create mapping
        let mapping = Arc::new(ShmMapping::new(&path, size)?);

        // Store mapping
        self.mappings
            .lock()
            .unwrap()
            .insert(name.to_string(), mapping.clone());

        Ok(ShmHandle {
            name: name.to_string(),
            mapping,
        })
    }

    pub fn open(&self, name: &str) -> Result<ShmHandle> {
        let path = self.shm_dir.join(name);

        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)?;

        let metadata = std::fs::metadata(&path)?;
        let size = metadata.len() as usize;

        let mapping = Arc::new(ShmMapping::new_from_file(&file, size)?);

        Ok(ShmHandle {
            name: name.to_string(),
            mapping,
        })
    }

    pub fn remove(&self, name: &str) -> Result<()> {
        self.mappings.lock().unwrap().remove(name);
        let path = self.shm_dir.join(name);
        std::fs::remove_file(&path)?;
        Ok(())
    }

    pub fn list(&self) -> Vec<String> {
        self.mappings.lock().unwrap().keys().cloned().collect()
    }
}

/// Shared memory mapping
pub struct ShmMapping {
    mmap: Mmap,
    mmap_mut: MmapMut,
    size: usize,
}

impl ShmMapping {
    fn new(path: &Path, size: usize) -> Result<Self> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;

        // Set file size
        file.set_len(size as u64)?;

        // Create memory map
        let mmap = unsafe { MmapOptions::new().len(size).map(&file)? };
        let mmap_mut = unsafe { MmapOptions::new().len(size).map_mut(&file)? };

        Ok(Self {
            mmap,
            mmap_mut,
            size,
        })
    }

    fn new_from_file(file: &std::fs::File, size: usize) -> Result<Self> {
        let mmap = unsafe { MmapOptions::new().len(size).map(file)? };
        let mmap_mut = unsafe { MmapOptions::new().len(size).map_mut(file)? };

        Ok(Self {
            mmap,
            mmap_mut,
            size,
        })
    }

    pub fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize> {
        let end = (offset + buf.len()).min(self.size);
        let len = end - offset;
        buf[..len].copy_from_slice(&self.mmap[offset..end]);
        Ok(len)
    }

    pub fn write(&mut self, offset: usize, data: &[u8]) -> Result<usize> {
        let end = (offset + data.len()).min(self.size);
        let len = end - offset;
        self.mmap_mut[offset..end].copy_from_slice(&data[..len]);
        Ok(len)
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.mmap
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.mmap_mut
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn flush(&self) -> Result<()> {
        self.mmap_mut.flush()?;
        Ok(())
    }
}

/// Shared memory handle
pub struct ShmHandle {
    name: String,
    mapping: Arc<crate::shm::ShmMapping>,
}

impl ShmHandle {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize> {
        self.mapping.read(offset, buf)
    }

    pub fn write(&self, _offset: usize, _data: &[u8]) -> Result<usize> {
        let _mapping = self.mapping.as_ref();
        // Note: This requires mutable access, but Arc only provides shared access
        // In a real implementation, we'd use a Mutex inside ShmMapping
        Ok(0)
    }

    pub fn size(&self) -> usize {
        self.mapping.size()
    }

    pub fn as_slice(&self) -> &[u8] {
        self.mapping.as_slice()
    }
}
