//! PN Next - Advanced architecture for PRoot Distro Next
//!
//! This crate provides the advanced architecture layer that builds upon
//! PRoot, PRoot-distro, and proot-compat to provide a more sophisticated,
//! user-friendly, and organized system for managing containers on Termux.

#![allow(deprecated)]
#![allow(future_incompatible)]

pub mod api;
pub mod backup_restore;
pub mod cluster;
pub mod container_socket_activation;
#[cfg(feature = "cri")]
pub mod cri_plugin;
pub mod daemon_runtime;
pub mod dbus;
pub mod gitops;
pub mod multiarch_build;
pub mod network;
pub mod plugin;
pub mod policy_engine;
pub mod resource_quota;
pub mod runtime;
pub mod scheduler;
pub mod security;
pub mod socket_activation;
pub mod storage;
pub mod structured_logging;
pub mod supervisor;
pub mod termux_boot;
pub mod wake_lock;
pub mod watchdog;
pub mod web_dashboard;
pub mod workflow;

pub use api::{ApiConfig, ApiServer, ApiState};
pub use backup_restore::{
    BackupConfig, BackupEvent, BackupManager, BackupManifest, BackupMetadata, BackupProgress,
    BackupStatus, BackupType, RestoreOptions,
};
pub use cluster::{
    ClusterConfig, ClusterEvent, ClusterManager, ClusterStateInfo, GossipConfig, NodeInfo,
    NodeResources, NodeRole, NodeStatus, PodSpec, SchedulerConfig as ClusterSchedulerConfig,
    SchedulingAlgorithm,
};
pub use container_socket_activation::{
    ContainerSocketConfig, ContainerSocketEvent, ContainerSocketManager,
    HealthCheckConfig as ContainerHealthCheckConfig, HealthCheckType,
};
#[cfg(feature = "cri")]
pub use cri_plugin::{CRIPluginConfig, CRIPluginManager};
pub use daemon_runtime::{
    DaemonEvent, DaemonHealthCheckConfig, DaemonHealthStatus, DaemonMessage, DaemonResponse,
    DaemonRestartPolicy, DaemonRuntime, DaemonRuntimeConfig, DaemonServiceState,
    DaemonServiceStatus, DaemonStatus, ManagedService,
};
pub use dbus::services::{
    interfaces, introspection, object_paths, service_names, ContainerConfig,
    ContainerInfo as DBusContainerInfo, MountConfig, NetworkConfig as DBusNetworkConfig,
    NetworkInfo, ResourceLimits as DBusResourceLimits, ServiceStatus as DBusServiceStatus,
    VolumeConfig, VolumeInfo,
};
pub use dbus::zbus_client::{create_container_connection, ContainerDBusClient, ZBusClient};
pub use dbus::{
    ConnectionInfo, DBusBroker, DBusBrokerConfig, DBusEvent, DBusMessage, MessageHeader,
    MessageType, ServiceRegistration,
};
pub use gitops::{
    Application, ApplicationDestination, ApplicationSource, GitOpsConfig, GitOpsEvent,
    GitOpsManager, HealthStatus as GitOpsHealthStatus, ResourceResult as GitOpsResourceResult,
    ResourceStatus as GitOpsResourceStatus, SyncPolicy, SyncStatus,
};
pub use multiarch_build::{
    BuildArtifact, BuildEvent, BuildManager, BuildMetadata,
    BuildProgress as MultiArchBuildProgress, BuildServiceConfig, BuildSpec, BuildStatus,
    PlatformResult, TargetArch,
};
pub use network::{NetworkConfig, NetworkInterface, NetworkManager};
pub use plugin::{Plugin, PluginConfig, PluginManager};
pub use policy_engine::{
    ComparisonOperator, EvaluationContext, EvaluationResult, HealthStatus as PolicyHealthStatus,
    PolicyAction, PolicyCondition, PolicyEffect, PolicyEngine, PolicyEngineConfig, PolicyRule,
    ResourceType as PolicyResourceType,
};
pub use resource_quota::{
    QuotaViolation, QuotaViolationAction, ResourceQuotaConfig, ResourceQuotaManager,
    ResourceQuotas, ResourceType, ResourceUsage,
};
pub use runtime::{Runtime, RuntimeConfig, RuntimeState};
pub use scheduler::{Scheduler, SchedulerConfig, Task, TaskPriority, TaskStatus};
pub use security::{Capability, Policy, SecurityConfig, SecurityManager};
pub use socket_activation::{
    SocketActivationConfig, SocketActivationEvent, SocketActivationManager, SocketConfig,
    SocketType,
};
pub use storage::{StorageConfig, StorageManager, Volume, VolumeDriver};
pub use structured_logging::{
    LogEntry, LogLevel, LogQuery, LogStats, RotationConfig, StructuredLogging,
    StructuredLoggingConfig,
};
pub use supervisor::{
    CircuitBreaker, HealthCheckConfig as SupervisorHealthCheckConfig, HealthStatus,
    LogRotationConfig, ProcessConfig, ResourceLimits, RestartCondition, RestartPolicy,
    ServiceConfig, ServiceMeta, ServiceState, ServiceStatus, ServiceSupervisor, ServiceType,
    StdioConfig, SupervisorBuilder, SupervisorConfig, SupervisorEvent, SupervisorState,
};
pub use termux_boot::{TermuxBootConfig, TermuxBootManager};
pub use wake_lock::{WakeLockConfig, WakeLockManager, WakeLockState, WakeLockType};
pub use watchdog::{
    EscalationAction, EscalationConfig, EscalationLevel, HealthCheckResult, MaxRestartsAction,
    RestartPolicy as WatchdogRestartPolicy, ServiceWatchdogState, WatchdogConfig, WatchdogEvent,
    WatchdogManager,
};
pub use web_dashboard::{
    ContainerInfo, DashboardEvent, DashboardState, PortInfo, ServiceInfo, SystemStats,
    WebDashboard, WebDashboardConfig,
};
pub use workflow::{Workflow, WorkflowConfig, WorkflowEngine, WorkflowStep as Step};

use anyhow::Result;
use dirs;
use pn_gateway::PnGateway;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// PN Next - Advanced architecture for PRoot Distro Next
pub struct PnNext {
    gateway: Arc<RwLock<Option<pn_gateway::PnGateway>>>,
    runtime: Arc<RwLock<Runtime>>,
    scheduler: Arc<RwLock<Scheduler>>,
    network: Arc<RwLock<NetworkManager>>,
    storage: Arc<RwLock<StorageManager>>,
    security: Arc<RwLock<SecurityManager>>,
    api: Arc<RwLock<ApiServer>>,
    workflow: Arc<RwLock<WorkflowEngine>>,
    plugin_manager: Arc<RwLock<PluginManager>>,
    supervisor: Arc<RwLock<ServiceSupervisor>>,
    config: PnNextConfig,
    state: tokio::sync::RwLock<PnNextState>,
}

impl PnNext {
    pub async fn new(config: PnNextConfig) -> Result<Self> {
        let gateway_config = pn_gateway::GatewayConfig {
            name: "pn-next-gateway".to_string(),
            version: "0.1.0".to_string(),
            ..Default::default()
        };

        let gateway = pn_gateway::PnGateway::new(gateway_config).await?;

        let runtime = Runtime::new(config.runtime.clone())?;
        let scheduler = Scheduler::new(config.scheduler.clone())?;
        let network = NetworkManager::new(config.network.clone())?;
        let storage = StorageManager::new(config.storage.clone())?;
        let security = SecurityManager::new(config.security.clone())?;
        let api = ApiServer::new(config.api.clone())?;
        let workflow = WorkflowEngine::new(config.workflow.clone())?;
        let plugin_manager = PluginManager::new(config.plugin.clone())?;

        // Create supervisor
        let supervisor_config = config
            .supervisor
            .clone()
            .unwrap_or_else(|| SupervisorConfig {
                services_dir: dirs::config_dir()
                    .unwrap_or_else(|| PathBuf::from("/etc/pn-next"))
                    .join("services"),
                runtime_dir: PathBuf::from("/run/pn-next"),
                log_dir: PathBuf::from("/var/log/pn-next"),
                ..Default::default()
            });
        let supervisor = SupervisorBuilder::new()
            .services_dir(supervisor_config.services_dir.clone())
            .runtime_dir(supervisor_config.runtime_dir.clone())
            .log_dir(supervisor_config.log_dir.clone())
            .build()?;

        Ok(Self {
            gateway: Arc::new(RwLock::new(Some(gateway))),
            runtime: Arc::new(RwLock::new(runtime)),
            scheduler: Arc::new(RwLock::new(scheduler)),
            network: Arc::new(RwLock::new(network)),
            storage: Arc::new(RwLock::new(storage)),
            security: Arc::new(RwLock::new(security)),
            api: Arc::new(RwLock::new(api)),
            workflow: Arc::new(RwLock::new(workflow)),
            plugin_manager: Arc::new(RwLock::new(plugin_manager)),
            supervisor: Arc::new(RwLock::new(supervisor)),
            config,
            state: tokio::sync::RwLock::new(PnNextState::Initializing),
        })
    }

    pub async fn initialize(&self) -> Result<()> {
        *self.state.write().await = PnNextState::Initializing;

        // Initialize gateway
        if let Some(gateway) = &mut *self.gateway.write().await {
            gateway.initialize().await?;
        }

        // Initialize components
        self.runtime.write().await.initialize().await?;
        self.scheduler.write().await.start().await?;
        self.network.write().await.initialize().await?;
        self.storage.write().await.initialize().await?;
        self.security.write().await.initialize().await?;
        self.api.write().await.start().await?;
        self.workflow.write().await.start().await?;
        self.plugin_manager.write().await.load_plugins().await?;

        // Initialize supervisor
        self.supervisor.write().await.initialize().await?;

        *self.state.write().await = PnNextState::Running;

        Ok(())
    }

    pub async fn run_container(&self, name: &str, image: &str, args: Vec<String>) -> Result<()> {
        // Validate with security manager
        self.security
            .read()
            .await
            .validate_container_creation(&name, &args)
            .await?;

        // Schedule the container creation
        let task = Task::new(
            format!("create_container_{}", name),
            serde_json::json!({
                "name": name,
                "image": image,
                "args": args,
            }),
            TaskPriority::Normal,
        );

        self.scheduler.write().await.submit(task).await?;

        Ok(())
    }

    pub async fn start_api(&self) -> Result<()> {
        self.api.write().await.start().await
    }

    pub async fn shutdown(&self) -> Result<()> {
        *self.state.write().await = PnNextState::ShuttingDown;

        // Shutdown in reverse order
        self.api.write().await.stop().await?;
        self.workflow.write().await.stop().await?;
        self.plugin_manager.write().await.unload_all().await?;

        // Shutdown supervisor
        (self.supervisor.write().await).shutdown().await?;

        self.security.write().await.shutdown().await?;
        self.storage.write().await.shutdown().await?;
        self.network.write().await.shutdown().await?;
        self.scheduler.write().await.stop().await?;
        self.runtime.write().await.shutdown().await?;

        if let Some(mut gateway) = self.gateway.write().await.take() {
            gateway.shutdown().await?;
        }

        *self.state.write().await = PnNextState::Stopped;

        Ok(())
    }

    pub async fn get_state(&self) -> PnNextState {
        self.state.read().await.clone()
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PnNextConfig {
    pub runtime: RuntimeConfig,
    pub scheduler: SchedulerConfig,
    pub network: NetworkConfig,
    pub storage: StorageConfig,
    pub security: SecurityConfig,
    pub api: ApiConfig,
    pub workflow: WorkflowConfig,
    pub plugin: PluginConfig,
    pub supervisor: Option<SupervisorConfig>,
}

impl Default for PnNextConfig {
    fn default() -> Self {
        Self {
            runtime: RuntimeConfig::default(),
            scheduler: SchedulerConfig::default(),
            network: NetworkConfig::default(),
            storage: StorageConfig::default(),
            security: SecurityConfig::default(),
            api: ApiConfig::default(),
            workflow: WorkflowConfig::default(),
            plugin: PluginConfig::default(),
            supervisor: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PnNextState {
    Initializing,
    Running,
    Degraded,
    ShuttingDown,
    Stopped,
    Error,
}

impl Default for PnNextState {
    fn default() -> Self {
        PnNextState::Initializing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_pn_next_creation() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = PnNextConfig {
            supervisor: Some(SupervisorConfig {
                services_dir: temp_dir.path().join("services"),
                runtime_dir: temp_dir.path().join("runtime"),
                log_dir: temp_dir.path().join("logs"),
                ..Default::default()
            }),
            ..Default::default()
        };
        let pn_next = PnNext::new(config).await;
        assert!(pn_next.is_ok());
    }
}
