//! Service Supervisor - runit/s6-style supervisor for container services
//!
//! Provides a process supervisor for managing container services with
//! declarative service definitions, dependency DAG, health checks,
//! and automatic restart policies.

/// Trace-only logging, off unless `PN_DEBUG` is set in the environment.
///
/// These lines were unconditional upstream and flooded real output — a single
/// `backup create` emitted ~280 kB of DEBUG for one alpine container, burying
/// the result. Enable with `PN_DEBUG=1` when diagnosing.
macro_rules! pn_trace {
    ($($arg:tt)*) => {
        if ::std::env::var_os("PN_DEBUG").is_some() {
            ::std::eprintln!($($arg)*);
        }
    };
}
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use futures::future::poll_fn;
use futures::Future;
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use petgraph::algo::toposort;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use reqwest;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::process::Command;
use tokio::sync::{broadcast, mpsc, oneshot, RwLock, Semaphore};
use tokio::time::timeout;
use tracing::{debug, error, info, instrument, warn};
use uuid::Uuid;

/// Service configuration loaded from TOML
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceConfig {
    /// Service metadata
    #[serde(flatten)]
    pub meta: ServiceMeta,

    /// Process configuration
    #[serde(default)]
    pub process: ProcessConfig,

    /// Resource limits
    #[serde(default)]
    pub resources: ResourceLimits,

    /// Restart policy
    #[serde(default)]
    pub restart: RestartPolicy,

    /// Health checks
    #[serde(default)]
    pub health: HealthCheckConfig,

    /// Dependencies
    #[serde(default)]
    pub dependencies: Vec<String>,

    /// Environment variables
    #[serde(default)]
    pub env: HashMap<String, String>,

    /// Working directory
    #[serde(default)]
    pub working_dir: Option<PathBuf>,

    /// User/group to run as
    #[serde(default)]
    pub user: Option<String>,

    /// Standard I/O redirection
    #[serde(default)]
    pub stdio: StdioConfig,

    /// Socket activation config
    #[serde(default)]
    pub sockets: Vec<SocketConfig>,
}

/// Service metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceMeta {
    /// Service name (unique identifier)
    pub name: String,

    /// Human-readable description
    #[serde(default)]
    pub description: String,

    /// Service version
    #[serde(default = "default_version")]
    pub version: String,

    /// Service type
    #[serde(default = "default_service_type")]
    pub service_type: ServiceType,

    /// Tags for grouping/filtering
    #[serde(default)]
    pub tags: Vec<String>,
}

fn default_version() -> String {
    "1.0.0".to_string()
}
fn default_service_type() -> ServiceType {
    ServiceType::Oneshot
}

/// Service type classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceType {
    /// Long-running daemon
    Daemon,
    /// One-shot task
    Oneshot,
    /// Socket-activated service
    Socket,
    /// Timer-based service
    Timer,
}

/// Process execution configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProcessConfig {
    /// Command to execute
    pub command: Vec<String>,

    /// Arguments (merged with command)
    #[serde(default)]
    pub args: Vec<String>,

    /// Pre-start script
    #[serde(default)]
    pub pre_start: Option<String>,

    /// Post-stop script
    #[serde(default)]
    pub post_stop: Option<String>,

    /// Pre-reload script
    #[serde(default)]
    pub pre_reload: Option<String>,

    /// Working directory override
    #[serde(default)]
    pub cwd: Option<PathBuf>,

    /// Umask
    #[serde(default = "default_umask")]
    pub umask: u32,

    /// Nice value (-20 to 19)
    #[serde(default)]
    pub nice: Option<i32>,

    /// CPU affinity mask
    #[serde(default)]
    pub cpu_affinity: Option<Vec<usize>>,
}

fn default_umask() -> u32 {
    0o022
}

/// Resource limits for the service
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResourceLimits {
    /// CPU limit (cores)
    #[serde(default)]
    pub cpu_limit: Option<f64>,

    /// Memory limit (bytes)
    #[serde(default)]
    pub memory_limit: Option<u64>,

    /// Memory swap limit (bytes)
    #[serde(default)]
    pub memory_swap_limit: Option<u64>,

    /// PIDs limit
    #[serde(default)]
    pub pids_limit: Option<u64>,

    /// CPU shares (weight)
    #[serde(default)]
    pub cpu_shares: Option<u64>,

    /// I/O weight
    #[serde(default)]
    pub io_weight: Option<u64>,

    /// Read BPS limit
    #[serde(default)]
    pub io_read_bps: Option<u64>,

    /// Write BPS limit
    #[serde(default)]
    pub io_write_bps: Option<u64>,
}

/// Restart policy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestartPolicy {
    /// Restart condition
    #[serde(default = "default_restart_condition")]
    pub condition: RestartCondition,

    /// Maximum restart attempts
    #[serde(default = "default_max_restarts")]
    pub max_restarts: u32,

    /// Restart window (seconds)
    #[serde(default = "default_restart_window")]
    pub restart_window_secs: u64,

    /// Base delay between restarts (seconds)
    #[serde(default = "default_restart_delay")]
    pub base_delay_secs: u64,

    /// Maximum delay between restarts (seconds)
    #[serde(default = "default_max_restart_delay")]
    pub max_delay_secs: u64,

    /// Exponential backoff multiplier
    #[serde(default = "default_backoff_multiplier")]
    pub backoff_multiplier: f64,
}

fn default_restart_condition() -> RestartCondition {
    RestartCondition::OnFailure
}
fn default_max_restarts() -> u32 {
    5
}
fn default_restart_window() -> u64 {
    60
}
fn default_restart_delay() -> u64 {
    1
}
fn default_max_restart_delay() -> u64 {
    60
}
fn default_backoff_multiplier() -> f64 {
    2.0
}
/// Circuit breaker for restart rate limiting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreaker {
    /// Number of failures in the current window
    pub failure_count: u32,
    /// Timestamp of the first failure in the current window
    pub window_start: Option<DateTime<Utc>>,
    /// Whether the circuit is open (blocking restarts)
    pub is_open: bool,
    /// Timestamp when the circuit was opened
    pub opened_at: Option<DateTime<Utc>>,
    /// Number of successful operations since last failure
    pub success_count: u32,
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self {
            failure_count: 0,
            window_start: None,
            is_open: false,
            opened_at: None,
            success_count: 0,
        }
    }
}

impl CircuitBreaker {
    /// Create a new circuit breaker
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a failure and check if circuit should open
    pub fn record_failure(&mut self, max_failures: u32, window_secs: u64) -> bool {
        let now = Utc::now();

        // Reset window if expired
        if let Some(window_start) = self.window_start {
            if now.signed_duration_since(window_start).num_seconds() > window_secs as i64 {
                self.failure_count = 0;
                self.window_start = None;
            }
        }

        if self.window_start.is_none() {
            self.window_start = Some(now);
        }

        self.failure_count += 1;
        self.success_count = 0;

        if self.failure_count >= max_failures && !self.is_open {
            self.is_open = true;
            self.opened_at = Some(now);
            true
        } else {
            false
        }
    }

    /// Record a success
    pub fn record_success(&mut self) {
        self.success_count += 1;
        // Half-open: allow trial after some successes
        if self.is_open && self.success_count >= 2 {
            self.is_open = false;
            self.failure_count = 0;
            self.window_start = None;
            self.opened_at = None;
        }
    }

    /// Check if circuit allows operation
    pub fn allows_operation(&self) -> bool {
        !self.is_open
    }

    /// Reset the circuit breaker
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Restart condition
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RestartCondition {
    /// Never restart
    Never,
    /// Restart on failure (non-zero exit)
    OnFailure,
    /// Restart on any exit
    Always,
    /// Restart unless explicitly stopped
    UnlessStopped,
    /// Restart on abnormal exit (signal)
    OnAbnormal,
}

impl Default for RestartPolicy {
    fn default() -> Self {
        Self {
            condition: RestartCondition::OnFailure,
            max_restarts: 5,
            restart_window_secs: 60,
            base_delay_secs: 1,
            max_delay_secs: 60,
            backoff_multiplier: 2.0,
        }
    }
}

/// Health check configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HealthCheckConfig {
    /// Enable health checks
    #[serde(default)]
    pub enabled: bool,

    /// Check interval (seconds)
    #[serde(default = "default_health_interval")]
    pub interval_secs: u64,

    /// Timeout for each check (seconds)
    #[serde(default = "default_health_timeout")]
    pub timeout_secs: u64,

    /// Start period before first check (seconds)
    #[serde(default = "default_health_start_period")]
    pub start_period_secs: u64,

    /// Number of consecutive failures before marking unhealthy
    #[serde(default = "default_health_retries")]
    pub retries: u32,

    /// Health check command
    #[serde(default)]
    pub command: Option<Vec<String>>,

    /// HTTP health check URL
    #[serde(default)]
    pub http_url: Option<String>,

    /// TCP health check port
    #[serde(default)]
    pub tcp_port: Option<u16>,
}

fn default_health_interval() -> u64 {
    30
}
fn default_health_timeout() -> u64 {
    10
}
fn default_health_start_period() -> u64 {
    10
}
fn default_health_retries() -> u32 {
    3
}

/// Standard I/O configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StdioConfig {
    /// Stdout log file
    #[serde(default)]
    pub stdout: Option<PathBuf>,

    /// Stderr log file
    #[serde(default)]
    pub stderr: Option<PathBuf>,

    /// Combine stdout/stderr
    #[serde(default)]
    pub combine: bool,

    /// Log rotation config
    #[serde(default)]
    pub rotation: Option<LogRotationConfig>,
}

/// Log rotation configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LogRotationConfig {
    /// Max file size (bytes)
    #[serde(default = "default_max_log_size")]
    pub max_size: u64,

    /// Max number of rotated files
    #[serde(default = "default_max_log_files")]
    pub max_files: u32,

    /// Compress rotated files
    #[serde(default)]
    pub compress: bool,
}

fn default_max_log_size() -> u64 {
    10 * 1024 * 1024
} // 10MB
fn default_max_log_files() -> u32 {
    5
}

/// Rotated log path for `idx` (`svc.log` -> `svc.log.1`).
fn rotated_log_path(path: &Path, idx: u32) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(format!(".{}", idx));
    PathBuf::from(s)
}

/// Size-triggered rotation honouring `LogRotationConfig`, applied at service
/// start. Previously `stdio.rotation` was parsed but never acted on, so
/// `stdout`/`stderr` files grew without bound.
/// ponytail: open-time only — a single long-lived process can still grow its
/// current file past `max_size` between restarts; add a stdout pump if that
/// matters.
fn rotate_log_if_needed(path: &Path, cfg: &LogRotationConfig) -> std::io::Result<()> {
    let len = match fs::metadata(path) {
        Ok(m) => m.len(),
        Err(_) => return Ok(()), // no file yet
    };
    if cfg.max_files == 0 || len < cfg.max_size {
        return Ok(());
    }
    // Shift `path.N-1` -> `path.N`, dropping the oldest.
    for i in (1..cfg.max_files).rev() {
        let from = rotated_log_path(path, i);
        if from.exists() {
            let _ = fs::rename(&from, rotated_log_path(path, i + 1));
        }
    }
    let _ = fs::rename(path, rotated_log_path(path, 1));
    Ok(())
}

/// Socket activation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocketConfig {
    /// Socket name
    pub name: String,

    /// Socket type
    #[serde(default = "default_socket_type")]
    pub socket_type: SocketType,

    /// Listen address
    pub listen: String,

    /// Backlog
    #[serde(default = "default_backlog")]
    pub backlog: i32,

    /// Accept connections (for systemd-style)
    #[serde(default)]
    pub accept: bool,
}

fn default_socket_type() -> SocketType {
    SocketType::Stream
}
fn default_backlog() -> i32 {
    128
}

/// Socket type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SocketType {
    Stream,
    Datagram,
    SeqPacket,
}

/// Service state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceState {
    /// Service is stopped
    Stopped,
    /// Service is starting
    Starting,
    /// Service is running
    Running,
    /// Service is stopping
    Stopping,
    /// Service failed
    Failed,
    /// Service is restarting
    Restarting,
    /// Service is reloading
    Reloading,
    /// Service is socket-activated (waiting)
    Listening,
}

/// Service status with runtime info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceStatus {
    /// Service name
    pub name: String,

    /// Current state
    pub state: ServiceState,

    /// Process ID (if running)
    pub pid: Option<u32>,

    /// Exit code (if stopped)
    pub exit_code: Option<i32>,

    /// Start time
    pub started_at: Option<DateTime<Utc>>,

    /// Last state change
    pub state_changed_at: DateTime<Utc>,

    /// Restart count in current window
    pub restart_count: u32,

    /// Total restarts
    pub total_restarts: u64,

    /// Last restart time
    pub last_restart_at: Option<DateTime<Utc>>,

    /// Health status
    pub health: Option<HealthStatus>,

    /// Resource usage
    pub resources: Option<ResourceUsage>,
}

/// Health status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    Healthy,
    Unhealthy,
    Starting,
    Unknown,
}

/// Resource usage snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceUsage {
    /// CPU usage (percent)
    pub cpu_percent: f64,

    /// Memory usage (bytes)
    pub memory_bytes: u64,

    /// Memory limit (bytes)
    pub memory_limit: Option<u64>,

    /// PIDs count
    pub pids: u64,

    /// Read bytes
    pub read_bytes: u64,

    /// Write bytes
    pub write_bytes: u64,
}

/// Service supervisor managing multiple services
pub struct ServiceSupervisor {
    config: SupervisorConfig,
    services: Arc<RwLock<HashMap<String, ManagedService>>>,
    dependency_graph: Arc<RwLock<DiGraph<String, ()>>>,
    event_tx: broadcast::Sender<SupervisorEvent>,
    shutdown_tx: Option<oneshot::Sender<()>>,
    state: Arc<RwLock<SupervisorState>>,
    /// Handle to the supervision loop task for graceful shutdown
    supervision_handle: Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
    /// Shutdown signal for supervision loop
    supervision_shutdown_tx: Arc<tokio::sync::Mutex<Option<oneshot::Sender<()>>>>,
}

/// Supervisor configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupervisorConfig {
    /// Service definitions directory
    pub services_dir: PathBuf,

    /// Runtime directory (pid files, sockets)
    pub runtime_dir: PathBuf,

    /// Log directory
    pub log_dir: PathBuf,

    /// Default restart policy
    #[serde(default)]
    pub default_restart: RestartPolicy,

    /// Default health check config
    #[serde(default)]
    pub default_health: HealthCheckConfig,

    /// Supervision interval (ms)
    #[serde(default = "default_supervision_interval")]
    pub supervision_interval_ms: u64,

    /// Graceful shutdown timeout (seconds)
    #[serde(default = "default_shutdown_timeout")]
    pub shutdown_timeout_secs: u64,

    /// Enable cgroups v2
    #[serde(default = "default_true")]
    pub enable_cgroups: bool,

    /// Max concurrent service operations
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent_ops: usize,
}

fn default_supervision_interval() -> u64 {
    1000
}
fn default_shutdown_timeout() -> u64 {
    30
}
fn default_true() -> bool {
    true
}
fn default_max_concurrent() -> usize {
    10
}

impl Default for SupervisorConfig {
    fn default() -> Self {
        Self {
            services_dir: PathBuf::from("/etc/pn-next/services"),
            runtime_dir: PathBuf::from("/run/pn-next"),
            log_dir: PathBuf::from("/var/log/pn-next"),
            default_restart: RestartPolicy::default(),
            default_health: HealthCheckConfig::default(),
            supervision_interval_ms: 1000,
            shutdown_timeout_secs: 30,
            enable_cgroups: true,
            max_concurrent_ops: 10,
        }
    }
}

/// Supervisor state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SupervisorState {
    Initializing,
    Running,
    Degraded,
    ShuttingDown,
    Stopped,
}

/// Managed service with runtime state
#[derive(Debug)]
struct ManagedService {
    config: ServiceConfig,
    status: ServiceStatus,
    process: Option<tokio::process::Child>,
    health_task: Option<tokio::task::JoinHandle<()>>,
    restart_semaphore: Arc<Semaphore>,
    cgroup_path: Option<PathBuf>,
    circuit_breaker: CircuitBreaker,
    /// Whether service was explicitly stopped by user
    user_stopped: bool,
}

/// Supervisor events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SupervisorEvent {
    ServiceStarted {
        name: String,
        pid: u32,
    },
    ServiceStopped {
        name: String,
        exit_code: Option<i32>,
    },
    ServiceFailed {
        name: String,
        error: String,
    },
    ServiceRestarting {
        name: String,
        attempt: u32,
    },
    ServiceHealthChanged {
        name: String,
        health: HealthStatus,
    },
    SupervisorStateChanged {
        state: SupervisorState,
    },
    ReloadRequested,
    ShutdownRequested,
}

impl ServiceSupervisor {
    /// Create a new service supervisor
    pub fn new(config: SupervisorConfig) -> Result<Self> {
        // Create directories
        fs::create_dir_all(&config.services_dir)?;
        fs::create_dir_all(&config.runtime_dir)?;
        fs::create_dir_all(&config.log_dir)?;

        let (event_tx, _) = broadcast::channel(1024);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        Ok(Self {
            config,
            services: Arc::new(RwLock::new(HashMap::new())),
            dependency_graph: Arc::new(RwLock::new(DiGraph::new())),
            event_tx,
            shutdown_tx: Some(shutdown_tx),
            state: Arc::new(RwLock::new(SupervisorState::Initializing)),
            supervision_handle: Arc::new(tokio::sync::Mutex::new(None)),
            supervision_shutdown_tx: Arc::new(tokio::sync::Mutex::new(None)),
        })
    }

    /// Get event receiver
    pub fn subscribe_events(&self) -> broadcast::Receiver<SupervisorEvent> {
        self.event_tx.subscribe()
    }

    /// Initialize and load services
    pub async fn initialize(&self) -> Result<()> {
        *self.state.write().await = SupervisorState::Initializing;

        // Load service definitions
        self.load_services().await?;

        // Build dependency graph
        self.build_dependency_graph().await?;

        // Validate dependencies
        self.validate_dependencies().await?;

        *self.state.write().await = SupervisorState::Running;
        self.emit(SupervisorEvent::SupervisorStateChanged {
            state: SupervisorState::Running,
        })
        .await;

        // Start supervision loop
        self.start_supervision_loop().await;

        info!(
            "Service supervisor initialized with {} services",
            self.services.read().await.len()
        );
        Ok(())
    }

    /// Load service definitions from TOML files
    pub async fn load_services(&self) -> Result<()> {
        let mut services = self.services.write().await;

        let entries = fs::read_dir(&self.config.services_dir)?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map_or(false, |ext| ext == "toml") {
                match Self::load_service_from_file(&path).await {
                    Ok(config) => {
                        let name = config.meta.name.clone();
                        let name_for_log = name.clone();
                        let managed = ManagedService {
                            config,
                            status: ServiceStatus {
                                name: name.clone(),
                                state: ServiceState::Stopped,
                                pid: None,
                                exit_code: None,
                                started_at: None,
                                state_changed_at: Utc::now(),
                                restart_count: 0,
                                total_restarts: 0,
                                last_restart_at: None,
                                health: Some(HealthStatus::Unknown),
                                resources: None,
                            },
                            process: None,
                            health_task: None,
                            restart_semaphore: Arc::new(Semaphore::new(1)),
                            cgroup_path: None,
                            circuit_breaker: CircuitBreaker::new(),
                            user_stopped: false,
                        };
                        services.insert(name, managed);
                        info!("Loaded service: {}", name_for_log);
                    }
                    Err(e) => {
                        error!("Failed to load service from {:?}: {}", path, e);
                    }
                }
            }
        }

        Ok(())
    }

    /// Load a single service from TOML file
    pub async fn load_service_from_file(path: &Path) -> Result<ServiceConfig> {
        let content = fs::read_to_string(path)?;
        let mut config: ServiceConfig =
            toml::from_str(&content).context("Failed to parse service TOML")?;

        // Validate required fields
        if config.meta.name.is_empty() {
            bail!("Service name is required");
        }

        if config.process.command.is_empty() {
            bail!("Service command is required");
        }

        // Set defaults
        if config.process.umask == 0 {
            config.process.umask = 0o022;
        }

        Ok(config)
    }

    /// Build dependency graph from service dependencies
    pub async fn build_dependency_graph(&self) -> Result<()> {
        let mut graph = DiGraph::new();
        let services = self.services.read().await;

        // Add nodes
        let mut node_map = HashMap::new();
        for name in services.keys() {
            let idx = graph.add_node(name.clone());
            node_map.insert(name.clone(), idx);
        }

        // Add edges
        for (name, service) in services.iter() {
            let from = node_map[name];
            for dep in &service.config.dependencies {
                if let Some(to) = node_map.get(dep) {
                    graph.add_edge(from, *to, ());
                } else {
                    warn!("Service '{}' depends on unknown service '{}'", name, dep);
                }
            }
        }

        *self.dependency_graph.write().await = graph;
        Ok(())
    }

    /// Validate dependency graph (no cycles)
    pub async fn validate_dependencies(&self) -> Result<()> {
        let graph = self.dependency_graph.read().await;

        match toposort(&*graph, None) {
            Ok(_) => Ok(()),
            Err(cycle) => {
                let node = cycle.node_id();
                let service = graph.node_weight(node).unwrap();
                bail!(
                    "Circular dependency detected involving service: {}",
                    service
                );
            }
        }
    }

    /// Start the supervision loop
    pub async fn start_supervision_loop(&self) -> Result<()> {
        let config = self.config.clone();
        let event_tx = self.event_tx.clone();
        let state = self.state.clone();
        let services = self.services.clone();
        let dependency_graph = self.dependency_graph.clone();
        let supervision_shutdown_tx = self.supervision_shutdown_tx.clone();

        // Create shutdown channel for this supervision loop
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel();
        {
            let mut guard = supervision_shutdown_tx.lock().await;
            *guard = Some(shutdown_tx);
        }

        tokio::spawn(async move {
            let mut ticker =
                tokio::time::interval(Duration::from_millis(config.supervision_interval_ms));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

            pn_trace!(
                "DEBUG: Supervision loop started, interval={}ms",
                config.supervision_interval_ms
            );

            // Create a future that polls the shutdown receiver
            let mut shutdown_fut =
                futures::future::poll_fn(move |cx| Pin::new(&mut shutdown_rx).poll(cx));

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        pn_trace!("DEBUG: Supervision loop tick");
                        if *state.read().await == SupervisorState::ShuttingDown {
                            pn_trace!("DEBUG: Supervision loop shutting down");
                            break;
                        }
                        // Run supervision tick
                        let to_check: Vec<_> = {
                            let services_guard = services.read().await;
                            services_guard
                                .iter()
                                .filter(|(_, s)| {
                                    matches!(
                                        s.status.state,
                                        ServiceState::Running | ServiceState::Starting | ServiceState::Restarting
                                    )
                                })
                                .map(|(n, s)| (n.clone(), s.config.clone()))
                                .collect()
                        };

                        for (name, config) in to_check {
                            // Check service health
                            let mut services_guard = services.write().await;
                            if let Some(managed) = services_guard.get_mut(&name) {
                                pn_trace!("DEBUG supervision_loop: name={}, state={:?}, process={:?}", name, managed.status.state, managed.process.is_some());
                                // Handle services that should be running but have no process (e.g., Restarting/Starting state)
                                if managed.process.is_none() && !managed.user_stopped && matches!(managed.status.state, ServiceState::Restarting | ServiceState::Starting) {
                                    // Only restart if this is a genuine restart (has been restarted before) or explicitly marked for restart
                                    // Skip initial startup - services in Starting state with no restarts yet are just starting for the first time
                                    let is_initial_start = managed.status.state == ServiceState::Starting && managed.status.total_restarts == 0 && managed.status.last_restart_at.is_none();
                                    if is_initial_start {
                                        // This is initial startup, let the normal start process handle it
                                        continue;
                                    }
                                    pn_trace!("DEBUG supervision_loop: Restarting service {} (state={:?})", name, managed.status.state);
                                    // Clone necessary data before releasing lock
                                    let service_config = config.clone();
                                    let event_tx_clone = event_tx.clone();
                                    let name_clone = name.clone();
                                    let services_for_start = services.clone();
                                    // Release lock before spawning task
                                    drop(services_guard);
                                    tokio::spawn(async move {
                                        // Build command
                                        let mut cmd = Command::new(&service_config.process.command[0]);
                                        if service_config.process.command.len() > 1 {
                                            cmd.args(&service_config.process.command[1..]);
                                        }
                                        cmd.args(&service_config.process.args);

                                        // Set environment
                                        for (k, v) in &service_config.env {
                                            cmd.env(k, v);
                                        }

                                        // Set working directory
                                        let cwd = service_config
                                            .working_dir
                                            .clone()
                                            .or_else(|| service_config.process.cwd.clone());
                                        if let Some(cwd) = &cwd {
                                            cmd.current_dir(cwd);
                                        }

                                        // Spawn process
                                        match cmd.spawn() {
                                            Ok(mut child) => {
                                                let pid = child.id().expect("Process ID should be available");
                                                let mut services = services_for_start.write().await;
                                                if let Some(managed) = services.get_mut(&name_clone) {
                                                    managed.process = Some(child);
                                                    managed.status.pid = Some(pid);
                                                    managed.status.state = ServiceState::Running;
                                                    managed.status.state_changed_at = Utc::now();
                                                    // Increment restart counters
                                                    managed.status.restart_count += 1;
                                                    managed.status.total_restarts += 1;
                                                    managed.status.last_restart_at = Some(Utc::now());
                                                }
                                                let _ = event_tx_clone.send(SupervisorEvent::ServiceStarted {
                                                    name: name_clone,
                                                    pid,
                                                });
                                            }
                                            Err(e) => {
                                                error!("Failed to start service {}: {}", name_clone, e);
                                                let mut services = services_for_start.write().await;
                                                if let Some(managed) = services.get_mut(&name_clone) {
                                                    managed.status.state = ServiceState::Failed;
                                                    managed.status.state_changed_at = Utc::now();
                                                }
                                                let _ = event_tx_clone.send(SupervisorEvent::ServiceFailed {
                                                    name: name_clone,
                                                    error: e.to_string(),
                                                });
                                            }
                                        }
                                    });
                                    continue;
                                }
                                if let Some(ref mut child) = managed.process {
                                    match child.try_wait() {
                                        Ok(Some(status)) => {
                                            let exit_code = status.code();
                                            managed.status.exit_code = exit_code;
                                            // A process that exited after an explicit user stop is
                                            // Stopped, not Failed: the stop is authoritative, the exit
                                            // code is incidental.
                                            let user_stopped = managed.user_stopped;
                                            managed.status.state = if user_stopped || exit_code == Some(0) {
                                                ServiceState::Stopped
                                            } else {
                                                ServiceState::Failed
                                            };
                                            managed.status.state_changed_at = Utc::now();

                                            let _ = event_tx.send(SupervisorEvent::ServiceStopped {
                                                name: name.clone(),
                                                exit_code,
                                            });

                                            // Handle restart policy
                                            if Self::should_restart_static(&managed.status, &config.restart, managed.user_stopped, &managed.circuit_breaker) {
                                                // Schedule the restart - clone services for the inner closure
                                                let name_clone = name.clone();
                                                let config_clone = config.clone();
                                                let event_tx_clone = event_tx.clone();
                                                let services_for_restart = services.clone();
                                                let config_for_restart = config.clone();
                                                let event_tx_for_restart = event_tx.clone();
                                                tokio::spawn(async move {
                                                    // Inline the schedule_restart logic here to avoid Clone issues
                                                    let circuit_open = {
                                                        let services = services_for_restart.read().await;
                                                        if let Some(managed) = services.get(&name_clone) {
                                                            let mut cb = managed.circuit_breaker.clone();
                                                            let window_secs = config_clone.restart.restart_window_secs;
                                                            let max_failures = config_clone.restart.max_restarts;
                                                            let opened = cb.record_failure(max_failures, window_secs);
                                                            if opened {
                                                                error!("Circuit breaker opened for service {}", name_clone);
                                                            }
                                                            !cb.allows_operation()
                                                        } else {
                                                            false
                                                        }
                                                    };

                                                    if circuit_open {
                                                        error!("Circuit breaker open for service {}, skipping restart", name_clone);
                                                        let mut services = services_for_restart.write().await;
                                                        if let Some(managed) = services.get_mut(&name_clone) {
                                                            if !managed.user_stopped {
                                                                managed.status.state = ServiceState::Failed;
                                                            }
                                                        }
                                                        let _ = event_tx_clone.send(SupervisorEvent::ServiceFailed {
                                                            name: name_clone.clone(),
                                                            error: "Circuit breaker open - too many failures".to_string(),
                                                        });
                                                        return;
                                                    }

                                                    let now = Utc::now();
                                                    let window_start = now - chrono::Duration::seconds(config_clone.restart.restart_window_secs as i64);

                                                    let recent_restarts = {
                                                        let services = services_for_restart.read().await;
                                                        if let Some(managed) = services.get(&name_clone) {
                                                            if managed
                                                                .status
                                                                .last_restart_at
                                                                .map_or(false, |t| t > window_start)
                                                            {
                                                                managed.status.restart_count + 1
                                                            } else {
                                                                1
                                                            }
                                                        } else {
                                                            1
                                                        }
                                                    };

                                                    if recent_restarts > config_clone.restart.max_restarts {
                                                        error!(
                                                            "Service {} exceeded max restarts ({}) in window",
                                                            name_clone, config_clone.restart.max_restarts
                                                        );
                                                        let mut services = services_for_restart.write().await;
                                                        if let Some(managed) = services.get_mut(&name_clone) {
                                                            if !managed.user_stopped {
                                                                managed.status.state = ServiceState::Failed;
                                                            }
                                                        }
                                                        let _ = event_tx_clone.send(SupervisorEvent::ServiceFailed {
                                                            name: name_clone.clone(),
                                                            error: format!(
                                                                "Exceeded max restarts ({}) in {}s window",
                                                                config_clone.restart.max_restarts, config_clone.restart.restart_window_secs
                                                            ),
                                                        });
                                                        return;
                                                    }

                                                    let delay = std::cmp::min(
                                                        (config_clone.restart.base_delay_secs as f64
                                                            * config_clone
                                                                .restart
                                                                .backoff_multiplier
                                                                .powi(recent_restarts as i32)) as u64,
                                                        config_clone.restart.max_delay_secs,
                                                    );

                                                    {
                                                        let mut services = services_for_restart.write().await;
                                                        if let Some(managed) = services.get_mut(&name_clone) {
                                                            // An explicit user stop pre-empts this in-flight
                                                            // restart decision: never resurrect a service the
                                                            // operator has stopped.
                                                            if managed.user_stopped
                                                                || matches!(
                                                                    managed.status.state,
                                                                    ServiceState::Stopping | ServiceState::Stopped
                                                                )
                                                            {
                                                                return;
                                                            }
                                                            managed.status.restart_count = recent_restarts;
                                                            managed.status.total_restarts += 1;
                                                            managed.status.last_restart_at = Some(Utc::now());
                                                            managed.status.state = ServiceState::Restarting;
                                                            managed.status.state_changed_at = Utc::now();
                                                        }
                                                    }

                                                    let _ = event_tx_clone.send(SupervisorEvent::ServiceRestarting {
                                                        name: name_clone.clone(),
                                                        attempt: recent_restarts,
                                                    });

                                                    tokio::time::sleep(Duration::from_secs(delay)).await;

                                                    // Actually restart by calling the instance method
                                                    // Note: We can't call self.start_service here since we don't have self
                                                    // The restart will be picked up by the next supervision tick
                                                    // when it sees the service in Restarting state
                                                });
                                            }
                                        }
                                        Ok(None) => {
                                            // Still running
                                        }
                                        Err(e) => {
                                            error!("Failed to check process status for {}: {}", name, e);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ = &mut shutdown_fut => {
                        pn_trace!("DEBUG: Supervision loop received shutdown signal");
                        break;
                    }
                }
            }
        });
        Ok(())
    }
    /// Determine if service should be restarted (static version for use in supervision loop)
    fn should_restart_static(
        status: &ServiceStatus,
        policy: &RestartPolicy,
        user_stopped: bool,
        circuit_breaker: &CircuitBreaker,
    ) -> bool {
        // Circuit breaker blocks restarts
        if !circuit_breaker.allows_operation() {
            return false;
        }

        match policy.condition {
            RestartCondition::Never => false,
            RestartCondition::Always => true,
            RestartCondition::OnFailure => status.exit_code.map_or(false, |c| c != 0),
            RestartCondition::OnAbnormal => {
                // OnAbnormal restarts on:
                // - Exit code < 0 or > 128 (some systems encode signals this way)
                // - Exit code is None (process terminated by signal on Unix)
                status.exit_code.map_or(true, |c| c < 0 || c > 128)
            }
            RestartCondition::UnlessStopped => !user_stopped,
        }
    }

    /// Single supervision tick
    pub async fn supervision_tick(
        &self,
        event_tx: &broadcast::Sender<SupervisorEvent>,
    ) -> Result<()> {
        let mut to_check = Vec::new();

        // Collect services to check (avoid holding lock during async ops)
        {
            let services_guard = self.services.read().await;
            for (name, service) in services_guard.iter() {
                if matches!(
                    service.status.state,
                    ServiceState::Running | ServiceState::Starting | ServiceState::Restarting
                ) {
                    to_check.push((name.clone(), service.config.clone()));
                }
            }
        }

        // Check each service
        for (name, config) in to_check {
            self.check_service_health(name, config, event_tx).await?;
        }
        Ok(())
    }

    /// Check service health and handle restarts
    pub async fn check_service_health(
        &self,
        name: String,
        config: ServiceConfig,
        event_tx: &broadcast::Sender<SupervisorEvent>,
    ) -> Result<()> {
        let mut services_guard = self.services.write().await;

        if let Some(managed) = services_guard.get_mut(&name) {
            pn_trace!(
                "DEBUG check_service_health: name={}, state={:?}, process={:?}",
                name,
                managed.status.state,
                managed.process.is_some()
            );
            // Handle services that should be running but have no process (e.g., Restarting/Starting state)
            if managed.process.is_none()
                && matches!(
                    managed.status.state,
                    ServiceState::Restarting | ServiceState::Starting
                )
            {
                // Try to restart the service
                pn_trace!(
                    "DEBUG: Restarting service {} (state={:?})",
                    name,
                    managed.status.state
                );
                drop(services_guard); // Release lock before calling start_service
                self.start_service(&name).await?;
                return Ok(());
            }

            // Check if process is still alive
            if let Some(ref mut child) = managed.process {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        // Process exited
                        let exit_code = status.code();
                        managed.status.exit_code = exit_code;
                        managed.status.state = if exit_code == Some(0) {
                            ServiceState::Stopped
                        } else {
                            ServiceState::Failed
                        };
                        managed.status.state_changed_at = Utc::now();

                        let _ = event_tx.send(SupervisorEvent::ServiceStopped {
                            name: name.clone(),
                            exit_code,
                        });

                        // Handle restart policy
                        if ServiceSupervisor::should_restart(
                            &managed.status,
                            &config.restart,
                            managed.user_stopped,
                            &managed.circuit_breaker,
                        ) {
                            self.schedule_restart(name.clone(), config.clone(), event_tx)
                                .await?;
                        }
                        Ok::<(), anyhow::Error>(())
                    }
                    Ok(None) => {
                        // Still running, check health
                        if config.health.enabled {
                            // Health check is done in separate task
                        }
                        Ok(())
                    }
                    Err(e) => {
                        error!("Failed to check process status for {}: {}", name, e);
                        Ok(())
                    }
                }?;
            }
        }
        Ok(())
    }

    /// Determine if service should be restarted
    pub fn should_restart(
        status: &ServiceStatus,
        policy: &RestartPolicy,
        user_stopped: bool,
        circuit_breaker: &CircuitBreaker,
    ) -> bool {
        // Circuit breaker blocks restarts
        if !circuit_breaker.allows_operation() {
            return false;
        }

        match policy.condition {
            RestartCondition::Never => false,
            RestartCondition::Always => true,
            RestartCondition::OnFailure => status.exit_code.map_or(false, |c| c != 0),
            RestartCondition::OnAbnormal => {
                // OnAbnormal restarts on:
                // - Exit code < 0 or > 128 (some systems encode signals this way)
                // - Exit code is None (process terminated by signal on Unix)
                status.exit_code.map_or(true, |c| c < 0 || c > 128)
            }
            RestartCondition::UnlessStopped => !user_stopped,
        }
    }

    /// Schedule a service restart
    pub async fn schedule_restart(
        &self,
        name: String,
        config: ServiceConfig,
        event_tx: &broadcast::Sender<SupervisorEvent>,
    ) -> Result<()> {
        // Check circuit breaker first
        let circuit_open = {
            let services = self.services.read().await;
            if let Some(managed) = services.get(&name) {
                let mut cb = managed.circuit_breaker.clone();
                // Record failure in circuit breaker
                let window_secs = config.restart.restart_window_secs;
                let max_failures = config.restart.max_restarts;
                let opened = cb.record_failure(max_failures, window_secs);
                if opened {
                    error!("Circuit breaker opened for service {}", name);
                }
                // Note: we can't update the circuit breaker here due to read lock
                // The check will be done in should_restart when actually restarting
                !cb.allows_operation()
            } else {
                false
            }
        };

        if circuit_open {
            error!(
                "Circuit breaker open for service {}, skipping restart",
                name
            );
            let mut services = self.services.write().await;
            if let Some(managed) = services.get_mut(&name) {
                managed.status.state = ServiceState::Failed;
            }
            let _ = event_tx.send(SupervisorEvent::ServiceFailed {
                name: name.clone(),
                error: "Circuit breaker open - too many failures".to_string(),
            });
            return Ok(());
        }

        let now = Utc::now();
        let window_start =
            now - chrono::Duration::seconds(config.restart.restart_window_secs as i64);

        // Count recent restarts - need to get current status
        let recent_restarts = {
            let services = self.services.read().await;
            if let Some(managed) = services.get(&name) {
                if managed
                    .status
                    .last_restart_at
                    .map_or(false, |t| t > window_start)
                {
                    managed.status.restart_count + 1
                } else {
                    1
                }
            } else {
                1
            }
        };

        if recent_restarts > config.restart.max_restarts {
            error!(
                "Service {} exceeded max restarts ({}) in window",
                name, config.restart.max_restarts
            );
            let mut services = self.services.write().await;
            if let Some(managed) = services.get_mut(&name) {
                if !managed.user_stopped {
                    managed.status.state = ServiceState::Failed;
                }
            }
            let _ = event_tx.send(SupervisorEvent::ServiceFailed {
                name: name.clone(),
                error: format!(
                    "Exceeded max restarts ({}) in {}s window",
                    config.restart.max_restarts, config.restart.restart_window_secs
                ),
            });
            return Ok(());
        }

        // Calculate backoff delay
        let delay = std::cmp::min(
            (config.restart.base_delay_secs as f64
                * config
                    .restart
                    .backoff_multiplier
                    .powi(recent_restarts as i32)) as u64,
            config.restart.max_delay_secs,
        );

        // Update restart count
        {
            let mut services = self.services.write().await;
            if let Some(managed) = services.get_mut(&name) {
                // An explicit user stop pre-empts this restart decision.
                if managed.user_stopped
                    || matches!(
                        managed.status.state,
                        ServiceState::Stopping | ServiceState::Stopped
                    )
                {
                    return Ok(());
                }
                managed.status.restart_count = recent_restarts;
                managed.status.total_restarts += 1;
                managed.status.last_restart_at = Some(Utc::now());
                managed.status.state = ServiceState::Restarting;
                managed.status.state_changed_at = Utc::now();
            }
        }

        let _ = event_tx.send(SupervisorEvent::ServiceRestarting {
            name: name.clone(),
            attempt: recent_restarts,
        });

        // Wait for delay
        tokio::time::sleep(Duration::from_secs(delay)).await;

        // Re-check: a user stop during the backoff must cancel the restart.
        if self
            .services
            .read()
            .await
            .get(&name)
            .map_or(false, |m| m.user_stopped)
        {
            return Ok(());
        }

        // Actually restart by calling the instance method
        let _ = self.start_service(&name).await?;
        Ok(())
    }

    /// Start a service
    #[instrument(skip(self))]
    pub async fn start_service(&self, name: &str) -> Result<()> {
        // Get service config and dependencies
        let (config, deps) = {
            let services = self.services.read().await;
            let service = services
                .get(name)
                .ok_or_else(|| anyhow::anyhow!("Service not found: {}", name))?;
            (service.config.clone(), service.config.dependencies.clone())
        };

        // Start dependencies first
        for dep in deps {
            let dep_status = {
                let services = self.services.read().await;
                services.get(&dep).map(|s| s.status.state)
            };

            if !matches!(dep_status, Some(ServiceState::Running)) {
                info!("Starting dependency: {}", dep);
                Box::pin(self.start_service(&dep)).await?;
            }
        }

        // Check if already running
        {
            let services = self.services.read().await;
            if let Some(managed) = services.get(name) {
                if matches!(
                    managed.status.state,
                    ServiceState::Running | ServiceState::Starting
                ) {
                    return Ok(());
                }
            }
        }

        // Execute pre-start script
        if let Some(pre_start) = &config.process.pre_start {
            info!("Running pre-start for {}", name);
            let output = Command::new("sh").arg("-c").arg(pre_start).output().await?;
            if !output.status.success() {
                bail!(
                    "Pre-start script failed for {}: {}",
                    name,
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }

        // Update state
        {
            let mut services = self.services.write().await;
            if let Some(managed) = services.get_mut(name) {
                managed.status.state = ServiceState::Starting;
                managed.status.state_changed_at = Utc::now();
            }
        }

        self.emit(SupervisorEvent::ServiceRestarting {
            name: name.to_string(),
            attempt: 0,
        })
        .await;

        // Build command
        let mut cmd = Command::new(&config.process.command[0]);
        if config.process.command.len() > 1 {
            cmd.args(&config.process.command[1..]);
        }
        cmd.args(&config.process.args);

        // Set environment
        for (k, v) in &config.env {
            cmd.env(k, v);
        }

        // Set working directory
        let cwd = config
            .working_dir
            .clone()
            .or_else(|| config.process.cwd.clone());
        if let Some(cwd) = &cwd {
            cmd.current_dir(cwd);
        }

        // Set user (if supported)
        if let Some(user) = &config.user {
            cmd.env("USER", user);
        }

        // Set up stdio
        if let Some(stdout_path) = &config.stdio.stdout {
            if let Some(rot) = &config.stdio.rotation {
                let _ = rotate_log_if_needed(stdout_path, rot);
            }
            let stdout_file = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(stdout_path)?;
            cmd.stdout(Stdio::from(stdout_file));
        } else {
            cmd.stdout(Stdio::piped());
        }

        if let Some(stderr_path) = &config.stdio.stderr {
            if let Some(rot) = &config.stdio.rotation {
                let _ = rotate_log_if_needed(stderr_path, rot);
            }
            let stderr_file = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(stderr_path)?;
            cmd.stderr(Stdio::from(stderr_file));
        } else if config.stdio.combine {
            cmd.stderr(Stdio::piped());
        } else {
            cmd.stderr(Stdio::piped());
        }

        // Spawn process
        let mut child = cmd
            .stdin(Stdio::null())
            .spawn()
            .context("Failed to spawn service process")?;

        let pid = child
            .id()
            .ok_or_else(|| anyhow::anyhow!("Failed to get PID"))?;

        // Update managed service
        {
            let mut services = self.services.write().await;
            if let Some(managed) = services.get_mut(name) {
                managed.process = Some(child);
                managed.status.pid = Some(pid);
                managed.status.state = ServiceState::Running;
                managed.status.started_at = Some(Utc::now());
                managed.status.state_changed_at = Utc::now();
                managed.status.exit_code = None;
                managed.status.restart_count = 0;
                managed.status.health = if config.health.enabled {
                    Some(HealthStatus::Starting)
                } else {
                    Some(HealthStatus::Unknown)
                };
                // Reset circuit breaker and user_stopped on successful start
                managed.circuit_breaker.reset();
                managed.user_stopped = false;
            }
        }

        // Setup cgroup if enabled
        if self.config.enable_cgroups {
            self.setup_cgroup(name, pid).await?;
        }

        // Start health check task
        if config.health.enabled {
            self.start_health_check(name.to_string(), config).await;
        }

        self.emit(SupervisorEvent::ServiceStarted {
            name: name.to_string(),
            pid,
        })
        .await;

        info!("Started service {} (PID: {})", name, pid);
        Ok(())
    }

    /// Setup cgroup for service
    async fn setup_cgroup(&self, name: &str, pid: u32) -> Result<()> {
        let cgroup_path = self.config.runtime_dir.join("cgroups").join(name);
        fs::create_dir_all(&cgroup_path)?;

        // Add PID to cgroup
        fs::write(cgroup_path.join("cgroup.procs"), pid.to_string())?;

        // Apply resource limits
        let services = self.services.read().await;
        if let Some(managed) = services.get(name) {
            let limits = &managed.config.resources;

            if let Some(cpu) = limits.cpu_limit {
                let quota = (cpu * 100000.0) as u64;
                fs::write(cgroup_path.join("cpu.max"), format!("{} 100000", quota))?;
            }

            if let Some(mem) = limits.memory_limit {
                fs::write(cgroup_path.join("memory.max"), mem.to_string())?;
            }

            if let Some(pids) = limits.pids_limit {
                fs::write(cgroup_path.join("pids.max"), pids.to_string())?;
            }

            // Store cgroup path
            drop(services);
            let mut services = self.services.write().await;
            if let Some(managed) = services.get_mut(name) {
                managed.cgroup_path = Some(cgroup_path);
            }
        }

        Ok(())
    }

    /// Start health check task
    async fn start_health_check(&self, name: String, config: ServiceConfig) {
        let services = self.services.clone();
        let event_tx = self.event_tx.clone();
        let health_config = config.health.clone();
        let name_for_task = name.clone();

        let handle = tokio::spawn(async move {
            // Wait for start period
            tokio::time::sleep(Duration::from_secs(health_config.start_period_secs)).await;

            let mut interval =
                tokio::time::interval(Duration::from_secs(health_config.interval_secs));
            let mut consecutive_failures = 0;

            loop {
                interval.tick().await;

                let is_healthy = ServiceSupervisor::run_health_check(&health_config).await;

                let mut services_guard = services.write().await;
                if let Some(managed) = services_guard.get_mut(&name_for_task) {
                    let new_health = if is_healthy {
                        consecutive_failures = 0;
                        HealthStatus::Healthy
                    } else {
                        consecutive_failures += 1;
                        if consecutive_failures >= health_config.retries {
                            HealthStatus::Unhealthy
                        } else {
                            HealthStatus::Starting
                        }
                    };

                    if managed.status.health != Some(new_health) {
                        managed.status.health = Some(new_health);
                        let _ = event_tx.send(SupervisorEvent::ServiceHealthChanged {
                            name: name_for_task.clone(),
                            health: new_health,
                        });
                    }

                    // If unhealthy and restart policy allows, trigger restart
                    if new_health == HealthStatus::Unhealthy {
                        if let Some(managed) = services_guard.get_mut(&name_for_task) {
                            if ServiceSupervisor::should_restart(
                                &managed.status,
                                &config.restart,
                                managed.user_stopped,
                                &managed.circuit_breaker,
                            ) {
                                managed.status.state = ServiceState::Restarting;
                            }
                        }
                    }
                }
            }
        });

        let mut services = self.services.write().await;
        if let Some(managed) = services.get_mut(&name) {
            managed.health_task = Some(handle);
        }
    }

    /// Run a single health check
    pub async fn run_health_check(config: &HealthCheckConfig) -> bool {
        // Command-based health check
        if let Some(cmd) = &config.command {
            if let Ok(output) = timeout(
                Duration::from_secs(config.timeout_secs),
                Command::new(&cmd[0]).args(&cmd[1..]).output(),
            )
            .await
            {
                if let Ok(output) = output {
                    return output.status.success();
                }
            }
            return false;
        }

        // HTTP health check
        if let Some(url) = &config.http_url {
            if let Ok(resp) = reqwest::Client::new()
                .get(url)
                .timeout(Duration::from_secs(config.timeout_secs))
                .send()
                .await
            {
                return resp.status().is_success();
            }
            return false;
        }

        // TCP health check
        if let Some(port) = config.tcp_port {
            if let Ok(_) = timeout(
                Duration::from_secs(config.timeout_secs),
                tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port)),
            )
            .await
            {
                return true;
            }
            return false;
        }

        true // No check configured = healthy
    }

    /// Stop a service
    #[instrument(skip(self))]
    pub async fn stop_service(&self, name: &str, force: bool) -> Result<()> {
        let (process, config) = {
            let mut services = self.services.write().await;
            let managed = services
                .get_mut(name)
                .ok_or_else(|| anyhow::anyhow!("Service not found: {}", name))?;

            if matches!(managed.status.state, ServiceState::Stopped) {
                return Ok(());
            }

            managed.status.state = ServiceState::Stopping;
            managed.status.state_changed_at = Utc::now();

            // Cancel health check
            if let Some(task) = managed.health_task.take() {
                task.abort();
            }

            (managed.process.take(), managed.config.clone())
        };

        if let Some(mut child) = process {
            // Execute pre-stop script
            if let Some(pre_stop) = &config.process.post_stop {
                let _ = Command::new("sh").arg("-c").arg(pre_stop).output().await;
            }

            // Send SIGTERM
            if let Some(pid) = child.id() {
                let _ = kill(Pid::from_raw(pid as i32), Signal::SIGTERM);
            }

            // Wait for graceful shutdown
            let timeout_duration = Duration::from_secs(self.config.shutdown_timeout_secs);
            let result = tokio::time::timeout(timeout_duration, child.wait()).await;

            let exit_code = match result {
                Ok(Ok(status)) => status.code(),
                Ok(Err(e)) => {
                    error!("Error waiting for service {}: {}", name, e);
                    None
                }
                Err(_) => {
                    // Timeout - force kill
                    pn_trace!("DEBUG: timeout reached, force={}", force);
                    if force {
                        if let Some(pid) = child.id() {
                            pn_trace!("DEBUG: sending SIGKILL to pid {}", pid);
                            let _ = kill(Pid::from_raw(pid as i32), Signal::SIGKILL);
                        }
                        pn_trace!(
                            "DEBUG: force killing service {}, waiting for exit code",
                            name
                        );
                        match child.wait().await {
                            Ok(status) => {
                                let code = status.code();
                                pn_trace!(
                                    "DEBUG: force killed service {} exit code: {:?}",
                                    name,
                                    code
                                );
                                code
                            }
                            Err(e) => {
                                error!("Error waiting for force-killed service {}: {}", name, e);
                                None
                            }
                        }
                    } else {
                        pn_trace!("DEBUG: force=false, returning None");
                        None
                    }
                }
            };

            // Update status
            let mut services = self.services.write().await;
            if let Some(managed) = services.get_mut(name) {
                managed.status.state = ServiceState::Stopped;
                managed.status.exit_code = exit_code;
                managed.status.state_changed_at = Utc::now();
                managed.status.pid = None;
                // Mark as user-stopped to prevent UnlessStopped restart
                // But don't mark as user_stopped if force kill and OnAbnormal policy (should restart)
                let is_force_kill_on_abnormal = force
                    && matches!(config.restart.condition, RestartCondition::OnAbnormal)
                    && exit_code.map_or(true, |c| c < 0 || c > 128);
                pn_trace!("DEBUG stop_service: name={}, force={}, exit_code={:?}, is_force_kill_on_abnormal={}", name, force, exit_code, is_force_kill_on_abnormal);
                managed.user_stopped = !is_force_kill_on_abnormal;

                // If force kill on OnAbnormal, trigger restart
                if is_force_kill_on_abnormal {
                    pn_trace!(
                        "DEBUG: Triggering restart for force kill on OnAbnormal: {}",
                        name
                    );
                    managed.status.state = ServiceState::Restarting;
                    managed.status.state_changed_at = Utc::now();
                    // Schedule restart
                    let name_clone = name.to_string();
                    let config_clone = config.clone();
                    let event_tx_clone = self.event_tx.clone();
                    let services_clone = self.services.clone();
                    let config_clone = config.clone();
                    let event_tx_clone = self.event_tx.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(Duration::from_secs(config.restart.base_delay_secs))
                            .await;
                        let mut services = services_clone.write().await;
                        if let Some(managed) = services.get_mut(&name_clone) {
                            managed.status.state = ServiceState::Starting;
                            managed.status.state_changed_at = Utc::now();
                        }
                        // The actual restart will be picked up by the next supervision tick
                        // when it sees the service in Restarting/Starting state
                    });
                }

                // Cleanup cgroup
                if let Some(cgroup) = &managed.cgroup_path {
                    let _ = fs::remove_dir_all(cgroup);
                }
            }

            self.emit(SupervisorEvent::ServiceStopped {
                name: name.to_string(),
                exit_code,
            })
            .await;

            info!("Stopped service {}", name);
        }

        Ok(())
    }

    /// Restart a service
    pub async fn restart_service(&self, name: &str) -> Result<()> {
        self.stop_service(name, false).await?;
        tokio::time::sleep(Duration::from_secs(1)).await;
        self.start_service(name).await
    }

    /// Reload a service (send SIGHUP)
    pub async fn reload_service(&self, name: &str) -> Result<()> {
        let (pid, config) = {
            let services = self.services.read().await;
            let managed = services
                .get(name)
                .ok_or_else(|| anyhow::anyhow!("Service not found: {}", name))?;
            (managed.status.pid, managed.config.clone())
        };

        if let Some(pid) = pid {
            // Execute pre-reload script
            if let Some(pre_reload) = &config.process.pre_reload {
                let _ = Command::new("sh").arg("-c").arg(pre_reload).output().await;
            }

            let _ = kill(Pid::from_raw(pid as i32), Signal::SIGHUP);

            info!("Reloaded service {} (PID: {})", name, pid);
        }

        Ok(())
    }

    /// Get service status
    pub async fn get_status(&self, name: &str) -> Result<ServiceStatus> {
        let services = self.services.read().await;
        services
            .get(name)
            .map(|s| s.status.clone())
            .ok_or_else(|| anyhow::anyhow!("Service not found: {}", name))
    }

    /// List all services
    pub async fn list_services(&self) -> Vec<ServiceStatus> {
        let services = self.services.read().await;
        services.values().map(|s| s.status.clone()).collect()
    }

    /// Get supervisor state
    pub async fn get_state(&self) -> SupervisorState {
        *self.state.read().await
    }

    /// Shutdown supervisor
    pub async fn shutdown(&mut self) -> Result<()> {
        *self.state.write().await = SupervisorState::ShuttingDown;
        self.emit(SupervisorEvent::SupervisorStateChanged {
            state: SupervisorState::ShuttingDown,
        })
        .await;

        // Signal supervision loop to stop
        let shutdown_tx = {
            let mut guard = self.supervision_shutdown_tx.lock().await;
            guard.take()
        };
        if let Some(tx) = shutdown_tx {
            let _ = tx.send(());
        }

        // Stop all services in reverse dependency order
        let stop_order = self.get_stop_order().await?;

        for name in stop_order {
            let _ = self.stop_service(&name, true).await;
        }

        *self.state.write().await = SupervisorState::Stopped;
        self.emit(SupervisorEvent::SupervisorStateChanged {
            state: SupervisorState::Stopped,
        })
        .await;

        Ok(())
    }

    /// Get stop order (reverse topological sort)
    async fn get_stop_order(&self) -> Result<Vec<String>> {
        let graph = self.dependency_graph.read().await;
        let topo =
            toposort(&*graph, None).map_err(|_| anyhow::anyhow!("Cycle in dependency graph"))?;

        let order: Vec<String> = topo
            .into_iter()
            .map(|idx| graph[idx].clone())
            .rev() // Reverse for stop order
            .collect();

        Ok(order)
    }

    /// Emit event
    pub async fn emit(&self, event: SupervisorEvent) {
        let _ = self.event_tx.send(event);
    }

    /// Enable a service (start on boot)
    pub async fn enable_service(&self, name: &str) -> Result<()> {
        // Create symlink in enabled directory
        let enabled_dir = self.config.services_dir.join("enabled");
        fs::create_dir_all(&enabled_dir)?;

        let target = self.config.services_dir.join(format!("{}.toml", name));
        let link = enabled_dir.join(format!("{}.toml", name));

        if link.exists() {
            fs::remove_file(&link)?;
        }

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&target, &link)?;
        }

        info!("Enabled service: {}", name);
        Ok(())
    }

    /// Disable a service
    pub async fn disable_service(&self, name: &str) -> Result<()> {
        let enabled_dir = self.config.services_dir.join("enabled");
        let link = enabled_dir.join(format!("{}.toml", name));

        if link.exists() {
            fs::remove_file(&link)?;
            info!("Disabled service: {}", name);
        }

        Ok(())
    }

    /// Check if service is enabled
    pub async fn is_enabled(&self, name: &str) -> bool {
        let enabled_dir = self.config.services_dir.join("enabled");
        let link = enabled_dir.join(format!("{}.toml", name));
        link.exists()
    }

    /// Reload configuration
    pub async fn reload(&self) -> Result<()> {
        info!("Reloading service supervisor configuration");

        // Reload service definitions
        self.load_services().await?;
        self.build_dependency_graph().await?;
        self.validate_dependencies().await?;

        self.emit(SupervisorEvent::ReloadRequested).await;

        Ok(())
    }

    /// Save service configuration to TOML file
    pub async fn save_service(&self, name: &str) -> Result<()> {
        let config = {
            let services = self.services.read().await;
            services
                .get(name)
                .map(|s| s.config.clone())
                .ok_or_else(|| anyhow::anyhow!("Service not found: {}", name))?
        };

        let path = self.config.services_dir.join(format!("{}.toml", name));
        let content =
            toml::to_string_pretty(&config).context("Failed to serialize service config")?;

        fs::write(&path, content)
            .with_context(|| format!("Failed to write service config to {:?}", path))?;

        info!("Saved service config: {}", name);
        Ok(())
    }

    /// Delete service configuration file
    pub async fn delete_service_config(&self, name: &str) -> Result<()> {
        let path = self.config.services_dir.join(format!("{}.toml", name));
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("Failed to delete service config: {:?}", path))?;
            info!("Deleted service config: {}", name);
        }

        // Remove from memory
        {
            let mut services = self.services.write().await;
            services.remove(name);
        }

        // Rebuild dependency graph
        self.build_dependency_graph().await?;
        self.validate_dependencies().await?;

        Ok(())
    }

    /// Create a new service from configuration
    pub async fn create_service(&self, config: ServiceConfig) -> Result<()> {
        let name = config.meta.name.clone();

        // Validate
        if config.meta.name.is_empty() {
            bail!("Service name is required");
        }
        if config.process.command.is_empty() {
            bail!("Service command is required");
        }

        // Save to file
        let path = self.config.services_dir.join(format!("{}.toml", name));
        let content =
            toml::to_string_pretty(&config).context("Failed to serialize service config")?;
        fs::write(&path, content)
            .with_context(|| format!("Failed to write service config to {:?}", path))?;

        // Load into memory
        let managed = ManagedService {
            config,
            status: ServiceStatus {
                name: name.clone(),
                state: ServiceState::Stopped,
                pid: None,
                exit_code: None,
                started_at: None,
                state_changed_at: Utc::now(),
                restart_count: 0,
                total_restarts: 0,
                last_restart_at: None,
                health: Some(HealthStatus::Unknown),
                resources: None,
            },
            process: None,
            health_task: None,
            restart_semaphore: Arc::new(Semaphore::new(1)),
            cgroup_path: None,
            circuit_breaker: CircuitBreaker::new(),
            user_stopped: false,
        };

        // Insert into memory (release lock before rebuilding dependency graph)
        {
            let mut services = self.services.write().await;
            services.insert(name.clone(), managed);
        }

        // Rebuild dependency graph (without holding the services write lock)
        self.build_dependency_graph().await?;
        self.validate_dependencies().await?;

        info!("Created service: {}", name);
        Ok(())
    }

    /// Get service configuration
    pub async fn get_service_config(&self, name: &str) -> Result<ServiceConfig> {
        let services = self.services.read().await;
        services
            .get(name)
            .map(|s| s.config.clone())
            .ok_or_else(|| anyhow::anyhow!("Service not found: {}", name))
    }

    /// Update service configuration
    pub async fn update_service(&self, name: &str, config: ServiceConfig) -> Result<()> {
        // Save to file
        self.save_service(name).await?;

        // Update in memory (release lock before rebuilding dependency graph)
        {
            let mut services = self.services.write().await;
            if let Some(managed) = services.get_mut(name) {
                managed.config = config;
            }
        }

        // Rebuild dependency graph (without holding the services write lock)
        self.build_dependency_graph().await?;
        self.validate_dependencies().await?;

        info!("Updated service: {}", name);
        Ok(())
    }
}

/// Supervisor builder for fluent configuration
pub struct SupervisorBuilder {
    config: SupervisorConfig,
}

impl SupervisorBuilder {
    pub fn new() -> Self {
        Self {
            config: SupervisorConfig::default(),
        }
    }

    pub fn services_dir<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.config.services_dir = path.as_ref().to_path_buf();
        self
    }

    pub fn runtime_dir<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.config.runtime_dir = path.as_ref().to_path_buf();
        self
    }

    pub fn log_dir<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.config.log_dir = path.as_ref().to_path_buf();
        self
    }

    pub fn supervision_interval(mut self, interval: Duration) -> Self {
        self.config.supervision_interval_ms = interval.as_millis() as u64;
        self
    }

    pub fn shutdown_timeout(mut self, timeout: Duration) -> Self {
        self.config.shutdown_timeout_secs = timeout.as_secs();
        self
    }

    pub fn enable_cgroups(mut self, enable: bool) -> Self {
        self.config.enable_cgroups = enable;
        self
    }

    pub fn max_concurrent_ops(mut self, max: usize) -> Self {
        self.config.max_concurrent_ops = max;
        self
    }

    pub fn default_restart(mut self, policy: RestartPolicy) -> Self {
        self.config.default_restart = policy;
        self
    }

    pub fn default_health(mut self, health: HealthCheckConfig) -> Self {
        self.config.default_health = health;
        self
    }

    pub fn build(self) -> Result<ServiceSupervisor> {
        ServiceSupervisor::new(self.config)
    }
}

impl Default for SupervisorBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_supervisor_creation() {
        let temp_dir = tempdir().unwrap();
        let config = SupervisorConfig {
            services_dir: temp_dir.path().join("services"),
            runtime_dir: temp_dir.path().join("runtime"),
            log_dir: temp_dir.path().join("logs"),
            ..Default::default()
        };

        let supervisor = ServiceSupervisor::new(config);
        assert!(supervisor.is_ok());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_service_config_parsing() {
        let toml = r#"
            name = "test-service"
            description = "Test service"
            version = "1.0.0"
            service_type = "daemon"

            [process]
            command = ["/bin/sleep", "100"]

            [restart]
            condition = "on-failure"
            max_restarts = 3

            [health]
            enabled = true
            interval_secs = 10
            command = ["/bin/true"]
            "#;

        let config: ServiceConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.meta.name, "test-service");
        assert_eq!(config.process.command, vec!["/bin/sleep", "100"]);
        assert_eq!(config.restart.condition, RestartCondition::OnFailure);
        assert!(config.health.enabled);
    }

    #[test]
    fn test_log_rotation_caps_file_count() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("svc.log");
        let cfg = LogRotationConfig {
            max_size: 16,
            max_files: 3,
            compress: false,
        };

        // Each round fills the base file past max_size; rotation must fire.
        for _ in 0..10 {
            fs::write(&path, vec![b'x'; 32]).unwrap();
            rotate_log_if_needed(&path, &cfg).unwrap();
        }

        // The full file was rotated out of the base path...
        assert!(!path.exists(), "full log rotated out of the base path");
        // ...and at most `max_files` rotated copies exist.
        let kept = (1..=cfg.max_files + 4)
            .map(|i| rotated_log_path(&path, i))
            .filter(|p| p.exists())
            .count();
        assert!(
            kept <= cfg.max_files as usize,
            "rotated files {kept} exceed max_files {}",
            cfg.max_files
        );
        assert!(
            rotated_log_path(&path, 1).exists(),
            "newest rotated file exists"
        );
    }
}

impl ServiceConfig {
    pub fn export_to_compose(&self) -> String {
        let mut out = String::from("# Generated by podnext\nversion: '3.8'\nservices:\n");
        out.push_str(&format!("  {}:\n", self.meta.name));
        if !self.process.command.is_empty() {
            out.push_str(&format!("    command: {:?}\n", self.process.command));
        }
        out
    }
}
