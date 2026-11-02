//! Container management for PRoot Distro
//!
//! Provides container lifecycle management with proot-core integration.

use crate::gpu::GpuConfig;
use crate::locking::LockManager;
use crate::paths::ContainerPaths;
use anyhow::Result;
use chrono::{DateTime, Utc};
use proot_core::{ProotConfig, ProotEngine};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Container manager
pub struct ContainerManager {
    containers: Arc<Mutex<HashMap<String, Container>>>,
    paths: ContainerPaths,
    lock_manager: Arc<LockManager>,
    proot_engine: Arc<Mutex<Option<ProotEngine>>>,
}

impl ContainerManager {
    pub fn new() -> Result<Self> {
        let manager = Self {
            containers: Arc::new(Mutex::new(HashMap::new())),
            paths: ContainerPaths::new()?,
            lock_manager: Arc::new(LockManager::new()?),
            proot_engine: Arc::new(Mutex::new(None)),
        };

        // Load existing containers from disk
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(manager.load_containers())
        })?;

        Ok(manager)
    }

    /// Load existing containers from metadata.json files
    async fn load_containers(&self) -> Result<()> {
        let containers_dir = self.paths.containers_dir.clone();
        if !containers_dir.exists() {
            return Ok(());
        }

        let mut containers = self.containers.lock().await;

        for entry in std::fs::read_dir(containers_dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();

            let metadata_path = entry.path().join("metadata.json");
            if metadata_path.exists() {
                if let Ok(metadata_str) = std::fs::read_to_string(&metadata_path) {
                    if let Ok(_metadata) = serde_json::from_str::<serde_json::Value>(&metadata_str)
                    {
                        let rootfs_path = entry.path().join("rootfs");
                        if rootfs_path.exists() {
                            let config = ContainerConfig {
                                name: name.clone(),
                                rootfs_path: Some(rootfs_path),
                                ..Default::default()
                            };

                            if let Ok(container) = Container::new(name.clone(), config) {
                                containers.insert(name, container);
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    async fn get_engine(&self) -> Result<Arc<Mutex<ProotEngine>>> {
        let mut engine_guard = self.proot_engine.lock().await;
        if engine_guard.is_none() {
            let config = ProotConfig::default();
            *engine_guard = Some(ProotEngine::new(config)?);
        }
        // Return a new Arc<Mutex<ProotEngine>> wrapping the same engine
        let engine = engine_guard.as_ref().unwrap();
        Ok(Arc::new(Mutex::new(engine.clone())))
    }

    pub async fn create(&self, name: &str, config: &ContainerConfig) -> Result<Container> {
        let mut containers = self.containers.lock().await;

        if containers.contains_key(name) {
            return Err(anyhow::anyhow!("Container '{}' already exists", name));
        }

        let container = Container::new(name.to_string(), config.clone())?;
        containers.insert(name.to_string(), container.clone());

        Ok(container)
    }

    pub async fn get(&self, name: &str) -> Result<Container> {
        let containers = self.containers.lock().await;
        containers
            .get(name)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Container '{}' not found", name))
    }

    pub async fn remove(&self, name: &str) -> Result<()> {
        let mut containers = self.containers.lock().await;
        containers
            .remove(name)
            .ok_or_else(|| anyhow::anyhow!("Container '{}' not found", name))?;
        Ok(())
    }

    pub async fn list(&self) -> Result<Vec<ContainerInfo>> {
        let containers = self.containers.lock().await;
        Ok(containers.values().map(|c| c.info()).collect())
    }

    pub async fn list_sessions(&self) -> Result<Vec<SessionInfo>> {
        // For now, return empty - sessions are managed by SessionManager
        Ok(Vec::new())
    }

    pub async fn kill(&self, name: &str) -> Result<()> {
        let mut containers = self.containers.lock().await;
        if let Some(container) = containers.get_mut(name) {
            container.status = ContainerStatus::Stopped;
            container.pid = None;
            Ok(())
        } else {
            Err(anyhow::anyhow!("Container '{}' not found", name))
        }
    }

    pub async fn start(&self, name: &str) -> Result<()> {
        let mut containers = self.containers.lock().await;
        let container = containers
            .get_mut(name)
            .ok_or_else(|| anyhow::anyhow!("Container '{}' not found", name))?;

        if container.status == ContainerStatus::Running {
            return Err(anyhow::anyhow!("Container '{}' is already running", name));
        }

        // Initialize proot engine if needed
        let engine = self.get_engine().await?;
        {
            let mut engine_guard = engine.lock().await;

            // Configure proot from container config
            engine_guard.config.rootfs = Some(container.rootfs_path.clone());
            engine_guard.config.bind = container
                .config
                .bind_mounts
                .iter()
                .map(|b| {
                    format!(
                        "{}:{}:{}",
                        b.source,
                        b.target,
                        if b.readonly { "ro" } else { "rw" }
                    )
                })
                .collect();
            engine_guard.config.cwd = Some(
                container
                    .config
                    .working_dir
                    .clone()
                    .unwrap_or_else(|| "/".to_string())
                    .into(),
            );
            engine_guard.config.env = container
                .config
                .env_vars
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect();
            engine_guard.config.command = if container.config.command.is_empty() {
                vec!["/bin/sh".to_string()]
            } else {
                container.config.command.clone()
            };
            engine_guard.config.seccomp = container.config.security.seccomp_profile.is_some();
            engine_guard.config.kernel_release = container.config.kernel_release.clone();
            engine_guard.config.hostname = container.config.hostname.clone();
        }

        // Start the container
        let pid = engine.lock().await.start().await?;

        container.status = ContainerStatus::Running;
        container.pid = Some(pid);

        Ok(())
    }

    pub async fn stop(&self, name: &str, timeout: u64) -> Result<()> {
        let mut containers = self.containers.lock().await;
        let container = containers
            .get_mut(name)
            .ok_or_else(|| anyhow::anyhow!("Container '{}' not found", name))?;

        if container.status != ContainerStatus::Running {
            return Err(anyhow::anyhow!("Container '{}' is not running", name));
        }

        if let Some(pid) = container.pid {
            // Send SIGTERM
            unsafe { libc::kill(pid as i32, libc::SIGTERM) };

            // Wait for process to exit
            let start = std::time::Instant::now();
            while start.elapsed().as_secs() < timeout {
                // Check if process is still alive
                if unsafe { libc::kill(pid as i32, 0) } != 0 {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }

            // Force kill if still alive
            if unsafe { libc::kill(pid as i32, 0) } == 0 {
                unsafe { libc::kill(pid as i32, libc::SIGKILL) };
            }
        }

        container.status = ContainerStatus::Stopped;
        container.pid = None;

        Ok(())
    }

    pub async fn restart(&self, name: &str, timeout: u64) -> Result<()> {
        self.stop(name, timeout).await?;
        self.start(name).await
    }

    /// Execute a command in the container and wait for completion (foreground execution)
    pub async fn execute(&self, name: &str, command: Vec<String>) -> Result<i32> {
        let container = self.get(name).await?;

        let engine = self.get_engine().await?;
        let mut engine_guard = engine.lock().await;

        // Configure proot from container config
        engine_guard.config.rootfs = Some(container.rootfs_path.clone());
        engine_guard.config.bind = container
            .config
            .bind_mounts
            .iter()
            .map(|b| {
                format!(
                    "{}:{}:{}",
                    b.source,
                    b.target,
                    if b.readonly { "ro" } else { "rw" }
                )
            })
            .collect();
        engine_guard.config.cwd = Some(
            container
                .config
                .working_dir
                .clone()
                .unwrap_or_else(|| "/".to_string())
                .into(),
        );
        engine_guard.config.env = container
            .config
            .env_vars
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        engine_guard.config.seccomp = container.config.security.seccomp_profile.is_some();
        engine_guard.config.kernel_release = container.config.kernel_release.clone();
        engine_guard.config.hostname = container.config.hostname.clone();

        // Handle empty command
        let cmd = if command.is_empty() {
            vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "exec /bin/sh".to_string(),
            ]
        } else {
            command
        };
        engine_guard.config.command = cmd;

        // Execute and wait for completion
        let exit_code = engine_guard.execute().await? as i32;
        Ok(exit_code)
    }

    pub fn backup(&self, _container: &Container, _dest: &Path) -> Result<()> {
        // Implementation for backing up container
        Ok(())
    }

    pub fn restore(&mut self, _name: &str, _src: &Path) -> Result<()> {
        // Implementation for restoring container
        Ok(())
    }
}

/// Container configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerConfig {
    pub name: String,
    pub distro: String,
    pub version: String,
    pub arch: String,
    pub rootfs_path: Option<PathBuf>,
    pub bind_mounts: Vec<BindMount>,
    pub env_vars: Vec<(String, String)>,
    pub command: Vec<String>,
    pub working_dir: Option<String>,
    pub kernel_release: Option<String>,
    pub hostname: Option<String>,
    pub resources: ResourceLimits,
    pub network: NetworkConfig,
    pub security: SecurityConfig,
    pub labels: HashMap<String, String>,
    pub annotations: HashMap<String, String>,
    pub gpu: Option<GpuConfig>,
}

impl Default for ContainerConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            distro: "ubuntu".to_string(),
            version: "22.04".to_string(),
            arch: "aarch64".to_string(),
            rootfs_path: None,
            bind_mounts: Vec::new(),
            env_vars: Vec::new(),
            command: Vec::new(),
            working_dir: None,
            kernel_release: None,
            hostname: None,
            resources: ResourceLimits::default(),
            network: NetworkConfig::default(),
            security: SecurityConfig::default(),
            labels: HashMap::new(),
            annotations: HashMap::new(),
            gpu: Some(GpuConfig::default()),
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

/// Container representation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Container {
    pub name: String,
    pub config: ContainerConfig,
    pub created_at: DateTime<Utc>,
    pub status: ContainerStatus,
    pub pid: Option<u32>,
    pub rootfs_path: PathBuf,
}

impl Container {
    pub fn new(name: String, config: ContainerConfig) -> Result<Self> {
        let rootfs_path = config.rootfs_path.clone().unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("/home/user"))
                .join(".pn-next")
                .join("containers")
                .join(&name)
        });

        Ok(Self {
            name,
            config,
            created_at: Utc::now(),
            status: ContainerStatus::Stopped,
            pid: None,
            rootfs_path,
        })
    }

    pub fn info(&self) -> ContainerInfo {
        ContainerInfo {
            name: self.name.clone(),
            distro: self.config.distro.clone(),
            version: self.config.version.clone(),
            arch: self.config.arch.clone(),
            status: self.status,
            created_at: self.created_at,
            rootfs_path: self.rootfs_path.clone(),
        }
    }
}

/// Container info for listing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerInfo {
    pub name: String,
    pub distro: String,
    pub version: String,
    pub arch: String,
    pub status: ContainerStatus,
    pub created_at: DateTime<Utc>,
    pub rootfs_path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContainerStatus {
    Stopped,
    Running,
    Paused,
    Error,
}

/// Session info for listing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub container_name: String,
    pub user: String,
    pub pid: Option<u32>,
    pub status: String,
    pub started_at: DateTime<Utc>,
}
