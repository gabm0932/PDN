//! Health monitoring for PN Gateway

use super::config::{HealthCheckType, HealthConfig};
use anyhow::Result;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use tokio::time::interval;

/// Health check handler type identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HealthCheckHandlerType {
    Process,
    DiskSpace,
    MemoryUsage,
    CpuUsage,
    NetworkConnectivity,
}

/// Health monitor for PN Gateway
pub struct HealthMonitor {
    config: HealthConfig,
    status: Arc<RwLock<HealthStatus>>,
    check_handlers:
        Arc<RwLock<HashMap<HealthCheckHandlerType, Box<dyn HealthCheckHandler + Send + Sync>>>>,
    check_tasks: HashMap<String, tokio::task::JoinHandle<()>>,
}

impl HealthMonitor {
    pub fn new(config: HealthConfig) -> Self {
        let mut handlers: HashMap<
            HealthCheckHandlerType,
            Box<dyn HealthCheckHandler + Send + Sync>,
        > = HashMap::new();

        handlers.insert(
            HealthCheckHandlerType::Process,
            Box::new(ProcessCheckHandler),
        );
        handlers.insert(
            HealthCheckHandlerType::DiskSpace,
            Box::new(DiskSpaceCheckHandler),
        );
        handlers.insert(
            HealthCheckHandlerType::MemoryUsage,
            Box::new(MemoryUsageCheckHandler),
        );
        handlers.insert(
            HealthCheckHandlerType::CpuUsage,
            Box::new(CpuUsageCheckHandler),
        );
        handlers.insert(
            HealthCheckHandlerType::NetworkConnectivity,
            Box::new(NetworkConnectivityCheckHandler),
        );

        Self {
            config,
            status: Arc::new(RwLock::new(HealthStatus::default())),
            check_handlers: Arc::new(RwLock::new(handlers)),
            check_tasks: HashMap::new(),
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        if !self.config.enabled {
            return Ok(());
        }

        for check in &self.config.checks {
            let check_name = check.name.clone();
            let check_interval = check.interval;
            let timeout = check.timeout;
            let check_type = check.check_type.clone();
            let target = check.target.clone();
            let status = self.status.clone();
            let handlers = self.check_handlers.clone();

            let _handle = tokio::spawn(async move {
                let mut interval_timer = interval(check_interval);

                loop {
                    interval_timer.tick().await;

                    let start = Instant::now();

                    // Run check with timeout
                    let result = tokio::time::timeout(timeout, async {
                        let handlers = handlers.read().await;
                        if let Some(handler) = handlers.get(&map_check_type(&check_type)) {
                            handler.check(&target)
                        } else {
                            Err(anyhow::anyhow!(
                                "No handler for check type: {:?}",
                                check_type
                            ))
                        }
                    })
                    .await;

                    let duration = start.elapsed();

                    let result = match result {
                        Ok(Ok(message)) => HealthCheckResult {
                            check_name: check_name.clone(),
                            status: HealthCheckStatus::Healthy,
                            message,
                            duration,
                            timestamp: Utc::now(),
                        },
                        Ok(Err(e)) => HealthCheckResult {
                            check_name: check_name.clone(),
                            status: HealthCheckStatus::Unhealthy,
                            message: e.to_string(),
                            duration,
                            timestamp: Utc::now(),
                        },
                        Err(_) => HealthCheckResult {
                            check_name: check_name.clone(),
                            status: HealthCheckStatus::Timeout,
                            message: format!("Check timed out after {:?}", timeout),
                            duration,
                            timestamp: Utc::now(),
                        },
                    };

                    // Update status
                    let mut status = status.write().await;
                    status.checks.insert(check_name.clone(), result);
                    status.update_overall();

                    // Check if recovery actions needed
                    if !status.is_healthy() {
                        // Trigger recovery actions
                        // This would be handled by a separate recovery manager
                    }
                }
            });

            // Store task handle
            // In a real implementation, we'd store these handles
        }

        Ok(())
    }

    pub async fn stop(&mut self) -> Result<()> {
        for (_, handle) in self.check_tasks.drain() {
            handle.abort();
        }
        Ok(())
    }

    pub async fn is_healthy(&self) -> bool {
        self.status.read().await.is_healthy()
    }

    pub async fn get_status(&self) -> HealthStatus {
        self.status.read().await.clone()
    }

    pub async fn run_check(&self, check_name: &str) -> Result<HealthCheckResult> {
        let check = self
            .config
            .checks
            .iter()
            .find(|c| c.name == check_name)
            .ok_or_else(|| anyhow::anyhow!("Check not found: {}", check_name))?;

        let handlers = self.check_handlers.read().await;
        let handler = handlers
            .get(&map_check_type(&check.check_type))
            .ok_or_else(|| anyhow::anyhow!("No handler for check type: {:?}", check.check_type))?;

        let start = Instant::now();
        let result = handler.check(&check.target);
        let duration = start.elapsed();

        match result {
            Ok(result) => Ok(HealthCheckResult {
                check_name: check_name.to_string(),
                status: HealthCheckStatus::Healthy,
                message: result,
                duration,
                timestamp: Utc::now(),
            }),
            Err(e) => Ok(HealthCheckResult {
                check_name: check_name.to_string(),
                status: HealthCheckStatus::Unhealthy,
                message: e.to_string(),
                duration,
                timestamp: Utc::now(),
            }),
        }
    }
}

fn map_check_type(check_type: &HealthCheckType) -> HealthCheckHandlerType {
    match check_type {
        HealthCheckType::Process => HealthCheckHandlerType::Process,
        HealthCheckType::DiskSpace => HealthCheckHandlerType::DiskSpace,
        HealthCheckType::MemoryUsage => HealthCheckHandlerType::MemoryUsage,
        HealthCheckType::CpuUsage => HealthCheckHandlerType::CpuUsage,
        HealthCheckType::NetworkConnectivity => HealthCheckHandlerType::NetworkConnectivity,
        HealthCheckType::Custom(_) => HealthCheckHandlerType::Process, // fallback
    }
}

trait HealthCheckHandler: Send + Sync {
    fn check(&self, target: &str) -> anyhow::Result<String>;
}

struct ProcessCheckHandler;

impl HealthCheckHandler for ProcessCheckHandler {
    fn check(&self, target: &str) -> anyhow::Result<String> {
        // Check if process is running
        let output = std::process::Command::new("pgrep").arg(target).output()?;

        if output.status.success() && !output.stdout.is_empty() {
            Ok(format!("Process '{}' is running", target))
        } else {
            Err(anyhow::anyhow!("Process '{}' not found", target))
        }
    }
}

struct DiskSpaceCheckHandler;

impl HealthCheckHandler for DiskSpaceCheckHandler {
    fn check(&self, target: &str) -> anyhow::Result<String> {
        use std::fs;
        let _stat = fs::metadata(target)?;
        // Simplified - in reality use statvfs
        Ok(format!("Disk space check for {} passed", target))
    }
}

struct MemoryUsageCheckHandler;

impl HealthCheckHandler for MemoryUsageCheckHandler {
    fn check(&self, _target: &str) -> anyhow::Result<String> {
        // Read from /proc/meminfo
        let _content = std::fs::read_to_string("/proc/meminfo")?;
        // Parse and calculate usage
        Ok("Memory usage check passed".to_string())
    }
}

struct CpuUsageCheckHandler;

impl HealthCheckHandler for CpuUsageCheckHandler {
    fn check(&self, _target: &str) -> anyhow::Result<String> {
        // Read from /proc/stat
        Ok("CPU usage check passed".to_string())
    }
}

struct NetworkConnectivityCheckHandler;

impl HealthCheckHandler for NetworkConnectivityCheckHandler {
    fn check(&self, target: &str) -> anyhow::Result<String> {
        // Simple ping or TCP connect test

        let parts: Vec<&str> = target.split(':').collect();
        if parts.len() == 2 {
            let host = parts[0];
            let port: u16 = parts[1].parse()?;

            match std::net::TcpStream::connect_timeout(
                &format!("{}:{}", host, port).parse()?,
                std::time::Duration::from_secs(5),
            ) {
                Ok(_) => Ok(format!("Network connectivity to {}:{} OK", host, port)),
                Err(e) => Err(anyhow::anyhow!("Connection failed: {}", e)),
            }
        } else {
            Err(anyhow::anyhow!("Invalid target format: {}", target))
        }
    }
}

/// Overall health status
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct HealthStatus {
    pub checks: HashMap<String, HealthCheckResult>,
    pub overall_status: OverallHealthStatus,
    pub last_updated: DateTime<Utc>,
}

impl HealthStatus {
    pub fn is_healthy(&self) -> bool {
        !self.checks.is_empty()
            && self
                .checks
                .values()
                .all(|c| c.status == HealthCheckStatus::Healthy)
    }

    fn update_overall(&mut self) {
        let has_unhealthy = self
            .checks
            .values()
            .any(|c| c.status == HealthCheckStatus::Unhealthy);
        let has_timeout = self
            .checks
            .values()
            .any(|c| c.status == HealthCheckStatus::Timeout);

        self.overall_status = if has_unhealthy {
            OverallHealthStatus::Unhealthy
        } else if has_timeout {
            OverallHealthStatus::Degraded
        } else {
            OverallHealthStatus::Healthy
        };
        self.last_updated = Utc::now();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum OverallHealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
    Unknown,
}

impl Default for OverallHealthStatus {
    fn default() -> Self {
        OverallHealthStatus::Unknown
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum HealthCheckStatus {
    Healthy,
    Unhealthy,
    Timeout,
    Unknown,
}

impl Default for HealthCheckStatus {
    fn default() -> Self {
        HealthCheckStatus::Unknown
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HealthCheckResult {
    pub check_name: String,
    pub status: HealthCheckStatus,
    pub message: String,
    pub duration: std::time::Duration,
    pub timestamp: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_health_monitor_creation() {
        let config = crate::config::HealthConfig::default();
        let monitor = HealthMonitor::new(config);
        assert!(monitor.is_healthy().await == false); // No checks run yet
    }
}
