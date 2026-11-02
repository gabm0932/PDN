//! Bindings management for PRoot Distro

use serde::{Deserialize, Serialize};

/// Bindings manager
pub struct BindingManager {
    binds: Vec<BindMount>,
}

impl BindingManager {
    pub fn new() -> Self {
        Self { binds: Vec::new() }
    }

    pub fn add_bind(&mut self, bind: BindMount) {
        self.binds.push(bind);
    }

    pub fn remove_bind(&mut self, target: &str) -> bool {
        self.binds.retain(|b| b.target != target);
        true
    }

    pub fn get_binds(&self) -> &[BindMount] {
        &self.binds
    }

    pub fn clear(&mut self) {
        self.binds.clear();
    }
}

/// Bind mount configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindMount {
    pub source: String,
    pub target: String,
    pub readonly: bool,
    pub recursive: bool,
    pub private: bool,
}

impl Default for BindMount {
    fn default() -> Self {
        Self {
            source: String::new(),
            target: String::new(),
            readonly: false,
            recursive: true,
            private: false,
        }
    }
}

/// Bind configuration for container
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindConfig {
    pub binds: Vec<BindMount>,
    pub tmpfs: Vec<TmpfsMount>,
    pub devices: Vec<DeviceMount>,
}

impl Default for BindConfig {
    fn default() -> Self {
        Self {
            binds: Vec::new(),
            tmpfs: Vec::new(),
            devices: Vec::new(),
        }
    }
}

/// Tmpfs mount configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TmpfsMount {
    pub target: String,
    pub size: Option<String>,
    pub mode: Option<u32>,
    pub options: Vec<String>,
}

/// Device mount configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceMount {
    pub source: String,
    pub target: String,
    pub permissions: String,
}
