//! Resource Quota Module
//! Enforces resource limits independent of container limits via supervisor

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use sysinfo::{Pid, Process, System};
use tokio::sync::{Mutex, RwLock};
use tokio::time::interval;
use tracing::{debug, error, info, warn};

/// Resource quota configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceQuotaConfig {
    /// Enable resource quota enforcement
    pub enabled: bool,
    /// Check interval (seconds)
    pub check_interval_secs: u64,
    /// Default quotas
    pub default_quotas: ResourceQuotas,
    /// Service-specific quotas
    pub service_quotas: HashMap<String, ResourceQuotas>,
    /// Action on quota violation
    pub violation_action: QuotaViolationAction,
    /// Grace period before action (seconds)
    pub grace_period_secs: u64,
    /// Enable cgroups v2 integration
    pub use_cgroups: bool,
    /// Cgroups mount point
    pub cgroups_mount: String,
}

impl Default for ResourceQuotaConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            check_interval_secs: 10,
            default_quotas: ResourceQuotas::default(),
            service_quotas: HashMap::new(),
            violation_action: QuotaViolationAction::Throttle,
            grace_period_secs: 30,
            use_cgroups: true,
            cgroups_mount: "/sys/fs/cgroup".to_string(),
        }
    }
}

/// Resource quotas for a service
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceQuotas {
    /// CPU quota (percentage * 100, e.g., 100 = 1 core, 200 = 2 cores)
    pub cpu_quota: Option<u64>,
    /// CPU period (microseconds, default 100000)
    pub cpu_period: Option<u64>,
    /// Memory limit (bytes)
    pub memory_limit: Option<u64>,
    /// Memory swap limit (bytes)
    pub memory_swap_limit: Option<u64>,
    /// Memory soft limit (bytes)
    pub memory_soft_limit: Option<u64>,
    /// Disk I/O weight (10-1000)
    pub io_weight: Option<u16>,
    /// Disk read bandwidth (bytes/sec)
    pub io_read_bps: Option<u64>,
    /// Disk write bandwidth (bytes/sec)
    pub io_write_bps: Option<u64>,
    /// Disk read IOPS
    pub io_read_iops: Option<u64>,
    /// Disk write IOPS
    pub io_write_iops: Option<u64>,
    /// Max processes
    pub pids_max: Option<u64>,
    /// Max open files
    pub nofile_limit: Option<u64>,
    /// Network bandwidth (bytes/sec)
    pub network_bps: Option<u64>,
}

impl Default for ResourceQuotas {
    fn default() -> Self {
        Self {
            cpu_quota: Some(100), // 1 core
            cpu_period: Some(100000),
            memory_limit: Some(512 * 1024 * 1024), // 512 MB
            memory_swap_limit: Some(1024 * 1024 * 1024), // 1 GB
            memory_soft_limit: Some(400 * 1024 * 1024), // 400 MB
            io_weight: Some(100),
            io_read_bps: None,
            io_write_bps: None,
            io_read_iops: None,
            io_write_iops: None,
            pids_max: Some(100),
            nofile_limit: Some(1024),
            network_bps: None,
        }
    }
}

/// Quota violation action
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuotaViolationAction {
    /// Log warning only
    Warn,
    /// Throttle resource usage
    Throttle,
    /// Restart service
    Restart,
    /// Stop service
    Stop,
    /// Quarantine service
    Quarantine,
}

/// Resource usage snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceUsage {
    /// Service name
    pub service: String,
    /// Timestamp
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// CPU usage (percentage * 100)
    pub cpu_usage: u64,
    /// Memory usage (bytes)
    pub memory_usage: u64,
    /// Memory swap usage (bytes)
    pub memory_swap_usage: u64,
    /// Disk read bytes/sec
    pub io_read_bps: u64,
    /// Disk write bytes/sec
    pub io_write_bps: u64,
    /// Disk read IOPS
    pub io_read_iops: u64,
    /// Disk write IOPS
    pub io_write_iops: u64,
    /// Number of processes
    pub pids_count: u64,
    /// Open file descriptors
    pub open_fds: u64,
    /// Network RX bytes/sec
    pub network_rx_bps: u64,
    /// Network TX bytes/sec
    pub network_tx_bps: u64,
}

/// Quota violation event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaViolation {
    /// Service name
    pub service: String,
    /// Resource type violated
    pub resource: ResourceType,
    /// Current usage
    pub current: u64,
    /// Quota limit
    pub limit: u64,
    /// Violation percentage
    pub percentage: f64,
    /// Timestamp
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Action taken
    pub action_taken: QuotaViolationAction,
}

/// Resource types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceType {
    Cpu,
    Memory,
    MemorySwap,
    IoReadBps,
    IoWriteBps,
    IoReadIops,
    IoWriteIops,
    Pids,
    Nofile,
    NetworkRx,
    NetworkTx,
}

/// Resource quota manager
/// Where service quotas live between commands.
///
/// Sits beside the other PDN runtime state so it is removed with the rest, and
/// follows the same `~/.podnext/` convention the container store uses.
fn quotas_path() -> Option<std::path::PathBuf> {
    dirs::home_dir().map(|home| home.join(".podnext").join("quotas.json"))
}

pub struct ResourceQuotaManager {
    config: ResourceQuotaConfig,
    usage_history: Arc<RwLock<HashMap<String, Vec<ResourceUsage>>>>,
    violations: Arc<RwLock<Vec<QuotaViolation>>>,
    event_sender: Option<tokio::sync::broadcast::Sender<QuotaViolation>>,
    sys: Arc<Mutex<System>>,
}

impl ResourceQuotaManager {
    /// Create a new resource quota manager
    pub fn new(config: ResourceQuotaConfig) -> Self {
        Self {
            config,
            usage_history: Arc::new(RwLock::new(HashMap::new())),
            violations: Arc::new(RwLock::new(Vec::new())),
            event_sender: None,
            sys: Arc::new(Mutex::new(System::new_all())),
        }
    }

    /// Set event broadcaster
    pub fn with_events(mut self, sender: tokio::sync::broadcast::Sender<QuotaViolation>) -> Self {
        self.event_sender = Some(sender);
        self
    }

    /// Get event receiver
    pub fn subscribe_events(&self) -> Option<tokio::sync::broadcast::Receiver<QuotaViolation>> {
        self.event_sender.as_ref().map(|s| s.subscribe())
    }

    /// Emit violation event
    fn emit(&self, violation: QuotaViolation) {
        if let Some(sender) = &self.event_sender {
            let _ = sender.send(violation.clone());
        }
    }

    /// Get quotas for a service
    pub fn get_quotas(&self, service: &str) -> ResourceQuotas {
        self.config
            .service_quotas
            .get(service)
            .cloned()
            .unwrap_or_else(|| self.config.default_quotas.clone())
    }

    /// Set quotas for a service, persisting the change.
    ///
    /// This used to write only into `config.service_quotas` in memory, and every
    /// CLI invocation builds a fresh manager — so `quota set` was discarded when
    /// the process exited and `quota get` always reported the defaults. A quota
    /// that cannot survive one command is not a quota.
    pub fn set_quotas(&mut self, service: &str, quotas: ResourceQuotas) -> Result<()> {
        self.config
            .service_quotas
            .insert(service.to_string(), quotas);
        self.save_quotas()
    }

    /// The service's own stored quota, or an all-unset one.
    ///
    /// Distinct from `get_quotas`, which falls back to the configured defaults:
    /// that is what you want when reporting the *effective* limit, but not when
    /// updating one field, or the update would materialise every default and
    /// wipe whatever the service already had.
    pub fn stored_quotas(&self, service: &str) -> ResourceQuotas {
        self.config
            .service_quotas
            .get(service)
            .cloned()
            .unwrap_or_default()
    }

    /// Every configured service quota, for `quota list`.
    ///
    /// `config` is private and there was no accessor, so `quota list` could not
    /// have shown anything even once quotas persisted.
    pub fn all_quotas(&self) -> std::collections::HashMap<String, ResourceQuotas> {
        self.config.service_quotas.clone()
    }

    /// Drop a service's quota and persist the removal.
    pub fn remove_quotas(&mut self, service: &str) -> Result<()> {
        self.config.service_quotas.remove(service);
        self.save_quotas()
    }

    /// Load persisted service quotas, replacing whatever is in memory.
    ///
    /// A missing file is not an error: it just means nothing has been set yet.
    pub fn load_quotas(&mut self) -> Result<()> {
        let Some(path) = quotas_path() else {
            return Ok(());
        };
        if !path.exists() {
            return Ok(());
        }

        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("reading quota store {}", path.display()))?;
        self.config.service_quotas = serde_json::from_str(&raw)
            .with_context(|| format!("parsing quota store {}", path.display()))?;
        Ok(())
    }

    /// Write the current service quotas to disk.
    pub fn save_quotas(&self) -> Result<()> {
        let Some(path) = quotas_path() else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let body = serde_json::to_string_pretty(&self.config.service_quotas)?;
        std::fs::write(&path, body)
            .with_context(|| format!("writing quota store {}", path.display()))?;
        Ok(())
    }

    /// Update resource usage for a service
    pub async fn update_usage(&self, service: &str, usage: ResourceUsage) -> Result<()> {
        if !self.config.enabled {
            return Ok(());
        }

        // Store in history
        let mut history = self.usage_history.write().await;
        let entries = history.entry(service.to_string()).or_insert_with(Vec::new);
        entries.push(usage.clone());

        // Keep only last 1000 entries
        if entries.len() > 1000 {
            entries.drain(0..entries.len() - 1000);
        }

        // Check quotas
        self.check_quotas(service, &usage).await?;

        Ok(())
    }

    /// Check quotas for a service
    async fn check_quotas(&self, service: &str, usage: &ResourceUsage) -> Result<()> {
        let quotas = self.get_quotas(service);
        let mut violations = Vec::new();

        // Check CPU
        if let Some(limit) = quotas.cpu_quota {
            if usage.cpu_usage > limit {
                let percentage = usage.cpu_usage as f64 / limit as f64 * 100.0;
                violations.push(QuotaViolation {
                    service: service.to_string(),
                    resource: ResourceType::Cpu,
                    current: usage.cpu_usage,
                    limit,
                    percentage,
                    timestamp: usage.timestamp,
                    action_taken: self.config.violation_action,
                });
            }
        }

        // Check Memory
        if let Some(limit) = quotas.memory_limit {
            if usage.memory_usage > limit {
                let percentage = usage.memory_usage as f64 / limit as f64 * 100.0;
                violations.push(QuotaViolation {
                    service: service.to_string(),
                    resource: ResourceType::Memory,
                    current: usage.memory_usage,
                    limit,
                    percentage,
                    timestamp: usage.timestamp,
                    action_taken: self.config.violation_action,
                });
            }
        }

        // Check Memory Swap
        if let Some(limit) = quotas.memory_swap_limit {
            if usage.memory_swap_usage > limit {
                let percentage = usage.memory_swap_usage as f64 / limit as f64 * 100.0;
                violations.push(QuotaViolation {
                    service: service.to_string(),
                    resource: ResourceType::MemorySwap,
                    current: usage.memory_swap_usage,
                    limit,
                    percentage,
                    timestamp: usage.timestamp,
                    action_taken: self.config.violation_action,
                });
            }
        }

        // Check I/O Read BPS
        if let Some(limit) = quotas.io_read_bps {
            if usage.io_read_bps > limit {
                let percentage = usage.io_read_bps as f64 / limit as f64 * 100.0;
                violations.push(QuotaViolation {
                    service: service.to_string(),
                    resource: ResourceType::IoReadBps,
                    current: usage.io_read_bps,
                    limit,
                    percentage,
                    timestamp: usage.timestamp,
                    action_taken: self.config.violation_action,
                });
            }
        }

        // Check I/O Write BPS
        if let Some(limit) = quotas.io_write_bps {
            if usage.io_write_bps > limit {
                let percentage = usage.io_write_bps as f64 / limit as f64 * 100.0;
                violations.push(QuotaViolation {
                    service: service.to_string(),
                    resource: ResourceType::IoWriteBps,
                    current: usage.io_write_bps,
                    limit,
                    percentage,
                    timestamp: usage.timestamp,
                    action_taken: self.config.violation_action,
                });
            }
        }

        // Check I/O Read IOPS
        if let Some(limit) = quotas.io_read_iops {
            if usage.io_read_iops > limit {
                let percentage = usage.io_read_iops as f64 / limit as f64 * 100.0;
                violations.push(QuotaViolation {
                    service: service.to_string(),
                    resource: ResourceType::IoReadIops,
                    current: usage.io_read_iops,
                    limit,
                    percentage,
                    timestamp: usage.timestamp,
                    action_taken: self.config.violation_action,
                });
            }
        }

        // Check I/O Write IOPS
        if let Some(limit) = quotas.io_write_iops {
            if usage.io_write_iops > limit {
                let percentage = usage.io_write_iops as f64 / limit as f64 * 100.0;
                violations.push(QuotaViolation {
                    service: service.to_string(),
                    resource: ResourceType::IoWriteIops,
                    current: usage.io_write_iops,
                    limit,
                    percentage,
                    timestamp: usage.timestamp,
                    action_taken: self.config.violation_action,
                });
            }
        }

        // Check PIDs
        if let Some(limit) = quotas.pids_max {
            if usage.pids_count > limit {
                let percentage = usage.pids_count as f64 / limit as f64 * 100.0;
                violations.push(QuotaViolation {
                    service: service.to_string(),
                    resource: ResourceType::Pids,
                    current: usage.pids_count,
                    limit,
                    percentage,
                    timestamp: usage.timestamp,
                    action_taken: self.config.violation_action,
                });
            }
        }

        // Check open files
        if let Some(limit) = quotas.nofile_limit {
            if usage.open_fds > limit {
                let percentage = usage.open_fds as f64 / limit as f64 * 100.0;
                violations.push(QuotaViolation {
                    service: service.to_string(),
                    resource: ResourceType::Nofile,
                    current: usage.open_fds,
                    limit,
                    percentage,
                    timestamp: usage.timestamp,
                    action_taken: self.config.violation_action,
                });
            }
        }

        // Check Network RX
        if let Some(limit) = quotas.network_bps {
            if usage.network_rx_bps > limit {
                let percentage = usage.network_rx_bps as f64 / limit as f64 * 100.0;
                violations.push(QuotaViolation {
                    service: service.to_string(),
                    resource: ResourceType::NetworkRx,
                    current: usage.network_rx_bps,
                    limit,
                    percentage,
                    timestamp: usage.timestamp,
                    action_taken: self.config.violation_action,
                });
            }
        }

        // Check Network TX
        if let Some(limit) = quotas.network_bps {
            if usage.network_tx_bps > limit {
                let percentage = usage.network_tx_bps as f64 / limit as f64 * 100.0;
                violations.push(QuotaViolation {
                    service: service.to_string(),
                    resource: ResourceType::NetworkTx,
                    current: usage.network_tx_bps,
                    limit,
                    percentage,
                    timestamp: usage.timestamp,
                    action_taken: self.config.violation_action,
                });
            }
        }

        // Process violations
        for violation in violations {
            self.handle_violation(&violation).await?;
        }

        Ok(())
    }

    /// Handle quota violation
    async fn handle_violation(&self, violation: &QuotaViolation) -> Result<()> {
        warn!(
            "Quota violation: service={}, resource={:?}, current={}, limit={}, percentage={:.1}%",
            violation.service,
            violation.resource,
            violation.current,
            violation.limit,
            violation.percentage
        );

        // Store violation
        let violation_clone = violation.clone();
        {
            let mut violations_guard = self.violations.write().await;
            violations_guard.push(violation_clone);
            // Keep only last 10000 violations
            let len = violations_guard.len();
            if len > 10000 {
                violations_guard.drain(0..len - 10000);
            }
        }

        // Emit event
        self.emit(violation.clone());

        // Take action based on configuration
        match self.config.violation_action {
            QuotaViolationAction::Warn => {
                // Just log warning (already done)
            }
            QuotaViolationAction::Throttle => {
                // Apply cgroups throttling
                self.apply_throttle(&violation.service, violation.resource)
                    .await?;
            }
            QuotaViolationAction::Restart => {
                // Would trigger service restart
                warn!(
                    "Would restart service {} due to quota violation",
                    violation.service
                );
            }
            QuotaViolationAction::Stop => {
                // Would stop service
                warn!(
                    "Would stop service {} due to quota violation",
                    violation.service
                );
            }
            QuotaViolationAction::Quarantine => {
                // Would quarantine service
                warn!(
                    "Would quarantine service {} due to quota violation",
                    violation.service
                );
            }
        }

        Ok(())
    }

    /// Apply throttling via cgroups
    async fn apply_throttle(&self, service: &str, resource: ResourceType) -> Result<()> {
        if !self.config.use_cgroups {
            return Ok(());
        }

        let cgroup_path = format!("{}/pdn/{}", self.config.cgroups_mount, service);

        match resource {
            ResourceType::Cpu => {
                // Write cpu.max
                let quotas = self.get_quotas(service);
                if let Some(quota) = quotas.cpu_quota {
                    if let Some(period) = quotas.cpu_period {
                        let content = format!("{} {}", quota, period);
                        tokio::fs::write(format!("{}/cpu.max", cgroup_path), content).await?;
                    }
                }
            }
            ResourceType::Memory => {
                // Write memory.max
                if let Some(limit) = self.get_quotas(service).memory_limit {
                    tokio::fs::write(format!("{}/memory.max", cgroup_path), limit.to_string())
                        .await?;
                }
            }
            ResourceType::IoReadBps | ResourceType::IoWriteBps => {
                // Write io.max (simplified)
                let quotas = self.get_quotas(service);
                let device = "8:0"; // Default block device, would need detection
                if let Some(read_bps) = quotas.io_read_bps {
                    let content = format!("{} rbps={}", device, read_bps);
                    tokio::fs::write(format!("{}/io.max", cgroup_path), content).await?;
                }
                if let Some(write_bps) = quotas.io_write_bps {
                    let content = format!("{} wbps={}", device, write_bps);
                    tokio::fs::write(format!("{}/io.max", cgroup_path), content).await?;
                }
            }
            ResourceType::Pids => {
                // Write pids.max
                if let Some(limit) = self.get_quotas(service).pids_max {
                    tokio::fs::write(format!("{}/pids.max", cgroup_path), limit.to_string())
                        .await?;
                }
            }
            _ => {
                // Other resources handled differently
            }
        }

        Ok(())
    }

    /// Get current usage for a service (from system)
    pub async fn collect_usage(&self, service: &str, pid: u32) -> Result<ResourceUsage> {
        let mut sys = self.sys.lock().await;
        sys.refresh_processes();

        let mut usage = ResourceUsage {
            service: service.to_string(),
            timestamp: chrono::Utc::now(),
            cpu_usage: 0,
            memory_usage: 0,
            memory_swap_usage: 0,
            io_read_bps: 0,
            io_write_bps: 0,
            io_read_iops: 0,
            io_write_iops: 0,
            pids_count: 0,
            open_fds: 0,
            network_rx_bps: 0,
            network_tx_bps: 0,
        };

        if let Some(process) = sys.process(Pid::from_u32(pid)) {
            usage.cpu_usage = (process.cpu_usage() * 100.0) as u64;
            usage.memory_usage = process.memory() * 1024; // Convert KB to bytes

            // Get open file descriptors (Linux-specific)
            #[cfg(target_os = "linux")]
            {
                if let Ok(entries) = std::fs::read_dir(format!("/proc/{}/fd", pid)) {
                    usage.open_fds = entries.count() as u64;
                }

                // Get child processes from /proc
                if let Ok(entries) = std::fs::read_dir(format!("/proc/{}/task", pid)) {
                    usage.pids_count = entries.count() as u64;
                } else {
                    usage.pids_count = 1;
                }
            }
        }

        Ok(usage)
    }

    /// Get usage history for a service
    pub async fn get_usage_history(
        &self,
        service: &str,
        limit: Option<usize>,
    ) -> Vec<ResourceUsage> {
        let history = self.usage_history.read().await;
        if let Some(entries) = history.get(service) {
            let limit = limit.unwrap_or(100);
            entries.iter().rev().take(limit).cloned().collect()
        } else {
            Vec::new()
        }
    }

    /// Get recent violations
    pub async fn get_violations(&self, limit: Option<usize>) -> Vec<QuotaViolation> {
        let violations = self.violations.read().await;
        let limit = limit.unwrap_or(100);
        violations.iter().rev().take(limit).cloned().collect()
    }

    /// Get violations for a specific service
    pub async fn get_service_violations(
        &self,
        service: &str,
        limit: Option<usize>,
    ) -> Vec<QuotaViolation> {
        let violations = self.violations.read().await;
        let limit = limit.unwrap_or(100);
        violations
            .iter()
            .rev()
            .filter(|v| v.service == service)
            .take(limit)
            .cloned()
            .collect()
    }

    /// Start monitoring loop
    pub async fn start_monitoring(&self) -> Result<()> {
        if !self.config.enabled {
            info!("Resource quota monitoring disabled");
            return Ok(());
        }

        let config = self.config.clone();
        let usage_history = self.usage_history.clone();
        let violations = self.violations.clone();
        let event_sender = self.event_sender.clone();
        let sys = self.sys.clone();

        tokio::spawn(async move {
            let manager = ResourceQuotaManager {
                config,
                usage_history,
                violations,
                event_sender,
                sys,
            };

            info!("Resource quota monitoring started");

            let mut interval = interval(Duration::from_secs(manager.config.check_interval_secs));

            loop {
                interval.tick().await;

                // In a real implementation, this would iterate over all tracked services
                // and collect their usage from the system
                debug!("Resource quota check tick");
            }
        });

        Ok(())
    }

    /// Shutdown
    pub async fn shutdown(&self) -> Result<()> {
        info!("Resource quota manager stopped");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_resource_quota_creation() {
        let config = ResourceQuotaConfig::default();
        let manager = ResourceQuotaManager::new(config);
        assert!(manager.config.enabled);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_quota_check() {
        let config = ResourceQuotaConfig {
            default_quotas: ResourceQuotas {
                memory_limit: Some(100 * 1024 * 1024), // 100 MB
                cpu_quota: Some(50),                   // 0.5 cores
                ..Default::default()
            },
            violation_action: QuotaViolationAction::Warn,
            ..Default::default()
        };

        let manager = ResourceQuotaManager::new(config);

        // Usage within limits
        let usage = ResourceUsage {
            service: "test".to_string(),
            timestamp: chrono::Utc::now(),
            cpu_usage: 30,
            memory_usage: 50 * 1024 * 1024,
            memory_swap_usage: 0,
            io_read_bps: 0,
            io_write_bps: 0,
            io_read_iops: 0,
            io_write_iops: 0,
            pids_count: 10,
            open_fds: 50,
            network_rx_bps: 0,
            network_tx_bps: 0,
        };

        manager.update_usage("test", usage).await.unwrap();

        // Usage exceeding memory limit
        let usage = ResourceUsage {
            service: "test".to_string(),
            timestamp: chrono::Utc::now(),
            cpu_usage: 30,
            memory_usage: 200 * 1024 * 1024, // Exceeds 100 MB
            memory_swap_usage: 0,
            io_read_bps: 0,
            io_write_bps: 0,
            io_read_iops: 0,
            io_write_iops: 0,
            pids_count: 10,
            open_fds: 50,
            network_rx_bps: 0,
            network_tx_bps: 0,
        };

        manager.update_usage("test", usage).await.unwrap();

        let violations = manager.get_violations(Some(10)).await;
        assert!(!violations.is_empty());
        assert_eq!(violations[0].resource, ResourceType::Memory);
    }
}
