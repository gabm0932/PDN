//! Container management for PRoot Distro
//!
//! Provides container lifecycle management with proot-core integration.

use crate::gpu::GpuConfig;
use crate::locking::LockManager;
use crate::paths::ContainerPaths;
use crate::runtime::{ProotBackend, RuntimeBackend, RuntimeKind, UmlBackend};
use anyhow::Result;
use chrono::{DateTime, Utc};
use proot_core::{ProotConfig, ProotEngine};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
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

    /// Load existing containers from disk, inspecting metadata.json and /etc/os-release
    async fn load_containers(&self) -> Result<()> {
        let containers_dir = self.paths.containers_dir.clone();
        if !containers_dir.exists() {
            return Ok(());
        }

        let mut containers = self.containers.lock().await;

        for entry in std::fs::read_dir(containers_dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            let rootfs_path = entry.path().join("rootfs");
            if !rootfs_path.is_dir() {
                continue;
            }

            // Only consider directories that have a usable rootfs
            let is_valid_rootfs = rootfs_path.join("etc").exists()
                || rootfs_path.join("bin").exists()
                || rootfs_path.join("usr").exists();
            if !is_valid_rootfs {
                continue;
            }

            let metadata_path = entry.path().join("metadata.json");
            let mut meta_distro = None;
            let mut meta_version = None;
            let mut created_at = None;

            if metadata_path.exists() {
                if let Ok(metadata_str) = std::fs::read_to_string(&metadata_path) {
                    if let Ok(metadata) = serde_json::from_str::<serde_json::Value>(&metadata_str) {
                        meta_distro = metadata
                            .get("image")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        meta_version = metadata
                            .get("tag")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        if let Some(ts_str) = metadata.get("created_at").and_then(|v| v.as_str()) {
                            if let Ok(dt) = DateTime::parse_from_rfc3339(ts_str) {
                                created_at = Some(dt.with_timezone(&Utc));
                            }
                        }
                    }
                }
            }

            // Inspect /etc/os-release in rootfs so DISTRO matches the real Linux distribution
            // (e.g. debian, ubuntu, alpine) rather than arbitrary container/target names.
            let (distro, version) = if let Some((os_id, os_ver)) = detect_os_release(&rootfs_path) {
                (os_id, os_ver)
            } else {
                (
                    meta_distro.unwrap_or_else(|| "unknown".to_string()),
                    meta_version.unwrap_or_else(|| "latest".to_string()),
                )
            };

            let config = ContainerConfig {
                name: name.clone(),
                distro,
                version,
                arch: std::env::consts::ARCH.to_string(),
                rootfs_path: Some(rootfs_path.clone()),
                ..Default::default()
            };

            if let Ok(mut container) = Container::new(name.clone(), config) {
                if let Some(created) = created_at {
                    container.created_at = created;
                } else if let Ok(dir_meta) = entry.metadata() {
                    if let Ok(ctime) = dir_meta.modified() {
                        container.created_at = DateTime::<Utc>::from(ctime);
                    }
                }
                containers.insert(name, container);
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
        // Terminate matching active disk sessions via SIGQUIT (graceful) then SIGKILL
        if let Ok(active) = crate::session::active_sessions() {
            for s in active {
                if s.container_name == name || name == "all" || name == "--all" {
                    let _ = crate::session::kill_session(s.pid, libc::SIGQUIT);
                }
            }
        }

        let mut containers = self.containers.lock().await;
        if let Some(container) = containers.get_mut(name) {
            if let Some(pid) = container.pid {
                // Only claim Stopped once the whole group is verifiably gone.
                if let Err(e) = terminate_group(pid, KILL_TIMEOUT_SECS).await {
                    container.status = ContainerStatus::Error;
                    return Err(e);
                }
                container.pid = None;
            }
            container.status = ContainerStatus::Stopped;
            Ok(())
        } else {
            Ok(())
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

            // Same fake /proc stubs as the interactive path (see execute).
            if let Some(container_dir) = container.rootfs_path.parent() {
                if crate::sysdata::ensure(container_dir).is_ok() {
                    engine_guard
                        .config
                        .bind
                        .extend(crate::sysdata::bindings(container_dir));
                }
                if crate::guestdirs::ensure(container_dir).is_ok() {
                    engine_guard
                        .config
                        .bind
                        .extend(crate::guestdirs::bindings(container_dir));
                }
            }
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
            // SIGTERM the group, escalate to SIGKILL, and verify. A survivor is
            // reported as an error rather than silently marked Stopped.
            if let Err(e) = terminate_group(pid, timeout).await {
                container.status = ContainerStatus::Error;
                return Err(e);
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

    /// Returns the execution backend corresponding to container's configured runtime
    pub fn get_backend(&self, container: &Container) -> Box<dyn RuntimeBackend> {
        match container.config.runtime {
            RuntimeKind::Proot => Box::new(ProotBackend::with_engine(self.proot_engine.clone())),
            RuntimeKind::Uml => Box::new(UmlBackend::new()),
        }
    }

    /// Execute a command in the container and wait for completion (foreground execution)
    pub async fn execute(&self, name: &str, command: Vec<String>) -> Result<i32> {
        let container = self.get(name).await?;
        let backend = self.get_backend(&container);
        backend.execute(&container, command).await
    }

    pub async fn execute_container(
        &self,
        container: &Container,
        command: Vec<String>,
    ) -> Result<i32> {
        let backend = self.get_backend(container);
        backend.execute(container, command).await
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
    #[serde(default)]
    pub runtime: RuntimeKind,
    #[serde(default)]
    pub isolated: bool,
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
            runtime: RuntimeKind::default(),
            isolated: false,
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
}

impl Default for Container {
    fn default() -> Self {
        Self {
            name: String::new(),
            config: ContainerConfig::default(),
            created_at: Utc::now(),
            status: ContainerStatus::Stopped,
            pid: None,
            rootfs_path: PathBuf::new(),
        }
    }
}

impl Container {
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

/// Detect Linux distribution ID and Version from rootfs (/etc/os-release or /usr/lib/os-release)
fn detect_os_release(rootfs: &Path) -> Option<(String, String)> {
    let candidates = [
        rootfs.join("etc/os-release"),
        rootfs.join("usr/lib/os-release"),
    ];

    for path in &candidates {
        if let Ok(content) = std::fs::read_to_string(path) {
            let mut id = None;
            let mut version_id = None;

            for line in content.lines() {
                let line = line.trim();
                if line.starts_with('#') || line.is_empty() {
                    continue;
                }
                if let Some((k, v)) = line.split_once('=') {
                    let k = k.trim();
                    let v = v.trim().trim_matches('"').trim_matches('\'');
                    if k == "ID" && id.is_none() {
                        id = Some(v.to_string());
                    } else if (k == "VERSION_ID" || k == "VERSION_CODENAME") && version_id.is_none()
                    {
                        version_id = Some(v.to_string());
                    }
                }
            }

            if let Some(id) = id {
                return Some((id, version_id.unwrap_or_else(|| "rolling".to_string())));
            }
        }
    }
    None
}

/// How long `kill()` waits for a group to die before escalating / giving up.
const KILL_TIMEOUT_SECS: u64 = 10;

/// Signal a whole process group and only return once it is verifiably gone.
///
/// proot traces the guest's descendants, so signalling the leader pid alone
/// orphans the tracees that hold the guest filesystem. The child was spawned
/// with `process_group(0)`, so its pgid equals its pid and `kill(-pgid, ..)`
/// reaches every descendant.
///
/// `kill(pid, 0)` is NOT a liveness test — it also succeeds for a zombie, which
/// made the old loop sleep the entire timeout and then SIGKILL a corpse. We reap
/// with waitpid and read process state from /proc instead.
async fn terminate_group(pgid: u32, timeout: u64) -> Result<()> {
    let group = -(pgid as i32);
    // SIGTERM the group (falls back to the bare pid for a non-leader/gone group;
    // ESRCH just means it already vanished and the poll below returns at once).
    unsafe { libc::kill(group, libc::SIGTERM) };

    if !wait_group_gone(pgid, timeout).await {
        unsafe { libc::kill(group, libc::SIGKILL) };
        if !wait_group_gone(pgid, timeout).await {
            anyhow::bail!("process group {} survived SIGKILL", pgid);
        }
    }
    Ok(())
}

/// Poll until the group has no live (non-zombie) member, up to `timeout` secs.
async fn wait_group_gone(pgid: u32, timeout: u64) -> bool {
    let start = std::time::Instant::now();
    loop {
        if !group_alive(pgid) {
            return true;
        }
        if start.elapsed().as_secs() >= timeout {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Is process group `pgid` still running (zombies excluded)?
fn group_alive(pgid: u32) -> bool {
    // Reap the leader if we are its parent and it already exited, so a zombie
    // does not read as alive.
    unsafe { libc::waitpid(pgid as i32, std::ptr::null_mut(), libc::WNOHANG) };
    live_pgrps().contains(&(pgid as i32))
}

/// pgrp of every live (non-zombie) process, parsed from /proc/<pid>/stat.
fn live_pgrps() -> Vec<i32> {
    let mut out = Vec::new();
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return out;
    };
    for entry in dir.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !name.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{}/stat", name)) else {
            continue;
        };
        // field 2 (comm) may contain spaces/parens; fields resume after the last ')'.
        // Fields: state ppid pgrp session ...
        let Some(close) = stat.rfind(')') else {
            continue;
        };
        let mut fields = stat[close + 1..].split_whitespace();
        let state = fields.next().unwrap_or("");
        if state.starts_with('Z') {
            continue;
        }
        let _ppid = fields.next();
        if let Some(pgrp) = fields.next().and_then(|s| s.parse::<i32>().ok()) {
            out.push(pgrp);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::CommandExt;

    fn count_group(pgid: u32) -> usize {
        live_pgrps()
            .into_iter()
            .filter(|p| *p == pgid as i32)
            .count()
    }

    /// A container whose process tree forks children must leave NOTHING behind
    /// after kill(). Observable assertion: no pid is left in the group.
    #[tokio::test]
    async fn kill_leaves_no_surviving_descendant() {
        // Stand-in for proot: a shell that forks two long sleepers and waits.
        let mut child = std::process::Command::new("sh")
            .arg("-c")
            .arg("sleep 300 & sleep 300 & wait")
            .process_group(0)
            .spawn()
            .expect("spawn sh");
        let pgid = child.id();

        // Wait (bounded) until the descendants have actually forked.
        let mut members = 0;
        for _ in 0..100 {
            members = count_group(pgid);
            if members >= 2 {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            members >= 2,
            "expected >=2 descendants in group {pgid}, saw {members}"
        );

        terminate_group(pgid, 5).await.expect("terminate group");

        let left = count_group(pgid);
        assert_eq!(
            left, 0,
            "{left} descendant(s) survived kill in group {pgid}"
        );
        let _ = child.wait(); // reap our own handle if not already reaped
    }

    /// A zombie satisfies kill(pid, 0), so the old wait loop could not break
    /// early and burned the full timeout. The new predicate reports it gone.
    #[test]
    fn zombie_is_not_reported_alive() {
        let mut child = std::process::Command::new("sh")
            .arg("-c")
            .arg("exit 0")
            .process_group(0)
            .spawn()
            .expect("spawn sh");
        let pid = child.id();

        // Wait until it has exited but is not reaped: a zombie.
        for _ in 0..200 {
            if let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) {
                if let Some(close) = stat.rfind(')') {
                    if stat[close + 1..].trim_start().starts_with('Z') {
                        break;
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        // Observable fact 1: the OLD liveness test calls a zombie "alive".
        assert_eq!(
            unsafe { libc::kill(pid as i32, 0) },
            0,
            "a zombie should satisfy kill(pid, 0)"
        );

        // Evidence (printed, never asserted): the old loop cannot break on a
        // zombie, so it runs the whole timeout; the new predicate returns now.
        let timeout = 1u64;
        let t0 = std::time::Instant::now();
        while t0.elapsed().as_secs() < timeout {
            if unsafe { libc::kill(pid as i32, 0) } != 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let old_elapsed = t0.elapsed();

        // Observable fact 2: the NEW predicate reaps/ignores zombies.
        let t1 = std::time::Instant::now();
        let alive = group_alive(pid);
        let new_elapsed = t1.elapsed();
        assert!(!alive, "a zombie must not count as a live process");

        eprintln!(
            "zombie pid {pid}: old kill(pid,0) loop = {old_elapsed:?} (full {timeout}s timeout), new predicate = {new_elapsed:?}"
        );
        let _ = child.wait();
    }
}
