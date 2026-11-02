//! Configuration for PRoot Distro

use crate::container::{ContainerConfig as ContainerContainerConfig, ContainerConfig};
use crate::image::ImageSource;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Distro type enumeration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DistroType {
    Ubuntu,
    Debian,
    Arch,
    Alpine,
    Fedora,
    OpenSUSE,
    CentOS,
    Custom(String),
}

impl Default for DistroType {
    fn default() -> Self {
        Self::Ubuntu
    }
}

impl std::fmt::Display for DistroType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DistroType::Custom(s) => write!(f, "{}", s),
            _ => write!(f, "{:?}", self),
        }
    }
}

/// Distro configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistroConfig {
    pub name: String,
    pub version: String,
    pub arch: String,
    pub variant: Option<String>,
    pub packages: Vec<String>,
    pub repositories: Vec<Repository>,
    pub environment: HashMap<String, String>,
    pub scripts: Scripts,
}

impl Default for DistroConfig {
    fn default() -> Self {
        Self {
            name: "ubuntu".to_string(),
            version: "22.04".to_string(),
            arch: "aarch64".to_string(),
            variant: None,
            packages: Vec::new(),
            repositories: Vec::new(),
            environment: HashMap::new(),
            scripts: Scripts::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repository {
    pub name: String,
    pub url: String,
    pub components: Vec<String>,
    pub signed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scripts {
    pub pre_install: Vec<String>,
    pub post_install: Vec<String>,
    pub pre_remove: Vec<String>,
    pub post_remove: Vec<String>,
}

impl Default for Scripts {
    fn default() -> Self {
        Self {
            pre_install: Vec::new(),
            post_install: Vec::new(),
            pre_remove: Vec::new(),
            post_remove: Vec::new(),
        }
    }
}

/// Install options
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallOptions {
    pub name: String,
    pub source: ImageSource,
    pub config: ContainerContainerConfig,
    pub distro_type: DistroType,
}

impl Default for InstallOptions {
    fn default() -> Self {
        Self {
            name: String::new(),
            source: ImageSource::Remote(String::new()),
            config: ContainerContainerConfig::default(),
            distro_type: DistroType::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindMount {
    pub source: String,
    pub target: String,
    pub readonly: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    pub cpu_limit: Option<f64>,
    pub memory_limit: Option<u64>,
    pub disk_limit: Option<u64>,
    pub pids_limit: Option<u64>,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            cpu_limit: None,
            memory_limit: None,
            disk_limit: None,
            pids_limit: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub mode: NetworkMode,
    pub bridge: Option<String>,
    pub ip_address: Option<String>,
    pub gateway: Option<String>,
    pub dns_servers: Vec<String>,
    pub port_mappings: Vec<PortMapping>,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            mode: NetworkMode::Bridge,
            bridge: Some("pn0".to_string()),
            ip_address: None,
            gateway: None,
            dns_servers: vec!["8.8.8.8".to_string(), "1.1.1.1".to_string()],
            port_mappings: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkMode {
    Bridge,
    Host,
    None,
    Container(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortMapping {
    pub host_port: u16,
    pub container_port: u16,
    pub protocol: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    pub privileged: bool,
    pub capabilities: Vec<String>,
    pub seccomp_profile: Option<String>,
    pub apparmor_profile: Option<String>,
    pub selinux_label: Option<String>,
    pub no_new_privileges: bool,
    pub read_only_rootfs: bool,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            privileged: false,
            capabilities: Vec::new(),
            seccomp_profile: None,
            apparmor_profile: None,
            selinux_label: None,
            no_new_privileges: true,
            read_only_rootfs: false,
        }
    }
}

/// Storage configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub driver: String,
    pub root_dir: PathBuf,
    pub volumes_dir: PathBuf,
    pub backup_dir: PathBuf,
    pub max_size: u64,
}

impl Default for StorageConfig {
    fn default() -> Self {
        let root = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("/home/user"))
            .join(".pn-next");
        Self {
            driver: "local".to_string(),
            root_dir: root.clone(),
            volumes_dir: root.join("volumes"),
            backup_dir: root.join("backups"),
            max_size: 50 * 1024 * 1024 * 1024, // 50 GB
        }
    }
}

/// Main configuration for PRoot Distro
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub distro: DistroConfig,
    pub container: ContainerConfig,
    pub network: NetworkConfig,
    pub storage: StorageConfig,
    pub security: SecurityConfig,
    pub resources: ResourceLimits,
    pub paths: PathsConfig,
    pub logging: LoggingConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            distro: DistroConfig::default(),
            container: ContainerConfig::default(),
            network: NetworkConfig::default(),
            storage: StorageConfig::default(),
            security: SecurityConfig::default(),
            resources: ResourceLimits::default(),
            paths: PathsConfig::default(),
            logging: LoggingConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathsConfig {
    pub root_dir: PathBuf,
    pub containers_dir: PathBuf,
    pub images_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub log_dir: PathBuf,
    pub config_dir: PathBuf,
}

impl Default for PathsConfig {
    fn default() -> Self {
        let root = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("/home/user"))
            .join(".pn-next");
        Self {
            root_dir: root.clone(),
            containers_dir: root.join("containers"),
            images_dir: root.join("images"),
            cache_dir: root.join("cache"),
            log_dir: root.join("logs"),
            config_dir: root.join("config"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
    pub format: String,
    pub output: Vec<String>,
    pub file_rotation: FileRotationConfig,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            format: "json".to_string(),
            output: vec!["stdout".to_string()],
            file_rotation: FileRotationConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileRotationConfig {
    pub enabled: bool,
    pub max_size: u64,
    pub max_files: u32,
    pub compress: bool,
}

impl Default for FileRotationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_size: 100 * 1024 * 1024, // 100 MB
            max_files: 10,
            compress: true,
        }
    }
}
