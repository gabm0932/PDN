//! Storage/Volume module - Stub for future implementation

use anyhow::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Volume configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VolumeConfig {
    pub name: String,
    pub driver: String,
    pub labels: std::collections::HashMap<String, String>,
    pub options: std::collections::HashMap<String, String>,
}

/// Volume information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeInfo {
    pub name: String,
    pub driver: String,
    pub mountpoint: PathBuf,
    pub labels: std::collections::HashMap<String, String>,
    pub options: std::collections::HashMap<String, String>,
    pub size: u64,
}

/// Root directory holding volume data.
///
/// The mountpoint used to be hardcoded to `/var/lib/podnext/volumes/<name>`,
/// which does not exist on Termux/Android (no writable `/var/lib`, no root), so
/// every volume advertised an unusable path. Keep the data beside the
/// containers instead, where the user can actually write.
fn volumes_root() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".podnext")
        .join("volumes")
}

/// Storage manager
pub struct StorageManager {
    volumes: HashMap<String, VolumeConfig>,
    config_path: PathBuf,
}

impl StorageManager {
    pub fn new() -> Result<Self, Error> {
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| crate::paths::temp_base())
            .join("podnext");
        std::fs::create_dir_all(&config_dir)?;
        let config_path = config_dir.join("volumes.toml");

        let mut manager = Self {
            volumes: HashMap::new(),
            config_path,
        };
        manager.load()?;
        Ok(manager)
    }

    fn load(&mut self) -> Result<()> {
        if self.config_path.exists() {
            let content = std::fs::read_to_string(&self.config_path)?;
            let volumes: HashMap<String, VolumeConfig> = toml::from_str(&content)?;
            self.volumes = volumes;
        }
        Ok(())
    }

    fn save(&self) -> Result<()> {
        let content = toml::to_string_pretty(&self.volumes)?;
        std::fs::write(&self.config_path, content)?;
        Ok(())
    }

    pub fn create_volume(&mut self, config: &VolumeConfig) -> Result<()> {
        if self.volumes.contains_key(&config.name) {
            return Err(Error::msg(format!(
                "Volume '{}' already exists",
                config.name
            )));
        }
        // Materialise the mountpoint: a volume the guest can bind into must
        // have a real directory to bind from.
        let mountpoint = volumes_root().join(&config.name);
        std::fs::create_dir_all(&mountpoint)?;

        self.volumes.insert(config.name.clone(), config.clone());
        self.save()?;
        Ok(())
    }

    pub fn remove_volume(&mut self, name: &str) -> Result<()> {
        if self.volumes.remove(name).is_some() {
            self.save()?;
            Ok(())
        } else {
            Err(Error::msg(format!("Volume '{}' not found", name)))
        }
    }

    pub fn list_volumes(&self) -> Result<Vec<VolumeInfo>> {
        let infos: Vec<VolumeInfo> = self
            .volumes
            .iter()
            .map(|(_, config)| VolumeInfo {
                name: config.name.clone(),
                driver: config.driver.clone(),
                mountpoint: volumes_root().join(&config.name),
                labels: config.labels.clone(),
                options: config.options.clone(),
                size: 0,
            })
            .collect();
        Ok(infos)
    }

    pub fn inspect_volume(&self, name: &str) -> Result<VolumeInfo> {
        if let Some(config) = self.volumes.get(name) {
            Ok(VolumeInfo {
                name: config.name.clone(),
                driver: config.driver.clone(),
                mountpoint: volumes_root().join(&config.name),
                labels: config.labels.clone(),
                options: config.options.clone(),
                size: 0,
            })
        } else {
            Err(Error::msg(format!("Volume '{}' not found", name)))
        }
    }
}
