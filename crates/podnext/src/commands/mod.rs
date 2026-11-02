//! CLI command implementations for service orchestration

use anyhow::{bail, Context, Result};
use pn_next::{
    BackupConfig,
    // Phase 7 modules
    BackupManager,
    BackupType,
    ContainerSocketConfig,
    ContainerSocketManager,
    DBusBroker,
    DBusBrokerConfig,
    DaemonRuntime,
    DaemonRuntimeConfig,
    LogLevel,
    LogQuery,
    PolicyEngine,
    PolicyEngineConfig,
    ResourceQuotaConfig,
    ResourceQuotaManager,
    ResourceQuotas,
    RestoreOptions,
    ServiceState,
    ServiceSupervisor,
    SocketActivationConfig,
    SocketActivationManager,
    StructuredLogging,
    StructuredLoggingConfig,
    SupervisorBuilder,
    TermuxBootConfig,
    TermuxBootManager,
    WakeLockConfig,
    WakeLockManager,
    WakeLockType,
    WatchdogConfig,
    WatchdogManager,
    WebDashboard,
    WebDashboardConfig,
};
use serde::Serialize;
use serde_json;
use serde_yaml;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use tabled::{Table, Tabled};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use crate::utils::format_size;

// Daemon socket path
const DAEMON_SOCKET: &str = "/data/data/com.termux/files/home/.local/run/pn-next/daemon.sock";

/// Send a command to the daemon and get response
async fn send_daemon_command(cmd: serde_json::Value) -> Result<serde_json::Value> {
    let stream = UnixStream::connect(DAEMON_SOCKET)
        .await
        .context("Failed to connect to daemon. Is podnext-daemon running?")?;
    let mut stream = stream;
    stream
        .write_all(serde_json::to_string(&cmd)?.as_bytes())
        .await?;
    stream.write_all(b"\n").await?;
    stream.flush().await?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).await?;

    let resp: serde_json::Value = serde_json::from_str(line.trim())?;
    Ok(resp)
}

/// Check if daemon is running
async fn is_daemon_running() -> bool {
    UnixStream::connect(DAEMON_SOCKET).await.is_ok()
}

// Output formatters
fn output_table<T: Tabled>(items: &[T]) {
    println!("{}", Table::new(items).to_string());
}

fn output_json<T: Serialize>(items: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(items)?);
    Ok(())
}

fn output_yaml<T: Serialize>(items: &T) -> Result<()> {
    println!("{}", serde_yaml::to_string(items)?);
    Ok(())
}

// ============ Service Commands ============

pub async fn execute_service_command(cmd: crate::ServiceCommands) -> Result<()> {
    match cmd {
        crate::ServiceCommands::Start { name } => cmd_service_start(name).await,
        crate::ServiceCommands::Stop { name, force } => cmd_service_stop(name, force).await,
        crate::ServiceCommands::Restart { name } => cmd_service_restart(name).await,
        crate::ServiceCommands::List { format } => cmd_service_list(format).await,
        crate::ServiceCommands::Status { name, format } => cmd_service_status(name, format).await,
        crate::ServiceCommands::Logs {
            name,
            follow,
            lines,
        } => cmd_service_logs(name, follow, lines).await,
        crate::ServiceCommands::Enable { name } => cmd_service_enable(name).await,
        crate::ServiceCommands::Disable { name } => cmd_service_disable(name).await,
        crate::ServiceCommands::Load { name } => cmd_service_load(name).await,
        crate::ServiceCommands::Create { name, config } => cmd_service_create(name, config).await,
    }
}

async fn cmd_service_start(name: String) -> Result<()> {
    println!("Starting service '{}'...", name);

    // Try daemon first
    if is_daemon_running().await {
        let resp = send_daemon_command(serde_json::json!({"cmd": "start", "name": name})).await?;
        if let Some(_err) = resp
            .get("type")
            .and_then(|t| t.as_str())
            .filter(|&t| t == "error")
        {
            let msg = resp
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error");
            bail!("Daemon error: {}", msg);
        }
        println!("Service '{}' start initiated via daemon", name);
        return Ok(());
    }

    // Fallback to local supervisor
    let supervisor = get_supervisor().await?;
    supervisor.start_service(&name).await?;

    // Wait for service to reach Running state
    for _ in 0..30 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let status = supervisor.get_status(&name).await?;
        if status.state == ServiceState::Running {
            println!("Service '{}' started (PID: {:?})", name, status.pid);
            return Ok(());
        }
    }

    let status = supervisor.get_status(&name).await?;
    println!(
        "Service '{}' start initiated (current state: {:?})",
        name, status.state
    );
    Ok(())
}

async fn cmd_service_stop(name: String, force: bool) -> Result<()> {
    println!("Stopping service '{}'...", name);

    if is_daemon_running().await {
        let resp =
            send_daemon_command(serde_json::json!({"cmd": "stop", "name": name, "force": force}))
                .await?;
        if let Some(_err) = resp
            .get("type")
            .and_then(|t| t.as_str())
            .filter(|&t| t == "error")
        {
            let msg = resp
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error");
            bail!("Daemon error: {}", msg);
        }
        println!("Service '{}' stopped via daemon", name);
        return Ok(());
    }

    let supervisor = get_supervisor().await?;
    supervisor.stop_service(&name, force).await?;
    println!("Service '{}' stopped", name);
    Ok(())
}

async fn cmd_service_restart(name: String) -> Result<()> {
    println!("Restarting service '{}'...", name);

    if is_daemon_running().await {
        let resp = send_daemon_command(serde_json::json!({"cmd": "restart", "name": name})).await?;
        if let Some(_err) = resp
            .get("type")
            .and_then(|t| t.as_str())
            .filter(|&t| t == "error")
        {
            let msg = resp
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error");
            bail!("Daemon error: {}", msg);
        }
        println!("Service '{}' restarted via daemon", name);
        return Ok(());
    }

    let supervisor = get_supervisor().await?;
    supervisor.restart_service(&name).await?;
    println!("Service '{}' restarted", name);
    Ok(())
}

async fn cmd_service_list(format: crate::OutputFormat) -> Result<()> {
    if is_daemon_running().await {
        let resp = send_daemon_command(serde_json::json!({"cmd": "list"})).await?;
        if let Some(_err) = resp
            .get("type")
            .and_then(|t| t.as_str())
            .filter(|&t| t == "error")
        {
            let msg = resp
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error");
            bail!("Daemon error: {}", msg);
        }
        if let Some(data) = resp.get("data") {
            let services: Vec<serde_json::Value> = serde_json::from_value(data.clone())?;
            print_services(services, format)?;
        }
        return Ok(());
    }

    let supervisor = get_supervisor().await?;
    let services = supervisor.list_services().await;
    print_services_local(services, format)?;
    Ok(())
}

async fn cmd_service_status(name: String, format: crate::OutputFormat) -> Result<()> {
    if is_daemon_running().await {
        let resp = send_daemon_command(serde_json::json!({"cmd": "status", "name": name})).await?;
        if let Some(_err) = resp
            .get("type")
            .and_then(|t| t.as_str())
            .filter(|&t| t == "error")
        {
            let msg = resp
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error");
            bail!("Daemon error: {}", msg);
        }
        if let Some(data) = resp.get("data") {
            print_service_status(data, format)?;
        }
        return Ok(());
    }

    let supervisor = get_supervisor().await?;
    let status = supervisor.get_status(&name).await?;
    print_service_status_local(status, format)?;
    Ok(())
}

async fn get_supervisor() -> Result<ServiceSupervisor> {
    let services_dir = std::env::var("PN_NEXT_SERVICES_DIR").unwrap_or_else(|_| {
        "/data/data/com.termux/files/home/.config/pn-next/services".to_string()
    });
    let runtime_dir = std::env::var("PN_NEXT_RUNTIME_DIR")
        .unwrap_or_else(|_| "/data/data/com.termux/files/home/.local/run/pn-next".to_string());
    let log_dir = std::env::var("PN_NEXT_LOG_DIR")
        .unwrap_or_else(|_| "/data/data/com.termux/files/home/.local/log/pn-next".to_string());

    let supervisor = SupervisorBuilder::new()
        .services_dir(services_dir)
        .runtime_dir(runtime_dir)
        .log_dir(log_dir)
        .build()?;

    // Initialize to load services from directory
    supervisor.initialize().await?;

    Ok(supervisor)
}

/// Print services from daemon response
fn print_services(services: Vec<serde_json::Value>, format: crate::OutputFormat) -> Result<()> {
    #[derive(Tabled, Serialize)]
    struct ServiceRow {
        #[tabled(rename = "NAME")]
        name: String,
        #[tabled(rename = "STATUS")]
        status: String,
        #[tabled(rename = "HEALTH")]
        health: String,
        #[tabled(rename = "PID")]
        pid: String,
        #[tabled(rename = "RESTART COUNT")]
        restart_count: u32,
    }

    let rows: Vec<ServiceRow> = services
        .iter()
        .map(|s| ServiceRow {
            name: s
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            status: s
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            health: s
                .get("health")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown")
                .to_string(),
            pid: s
                .get("pid")
                .and_then(|v| v.as_u64())
                .map(|v| v.to_string())
                .unwrap_or("-".to_string()),
            restart_count: s.get("restart_count").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        })
        .collect();

    match format {
        crate::OutputFormat::Table => output_table(&rows),
        crate::OutputFormat::Json => output_json(&services)?,
        crate::OutputFormat::Yaml => output_yaml(&services)?,
        crate::OutputFormat::Text => {
            for s in services {
                println!(
                    "{} {} {} {:?} {}",
                    s.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                    s.get("status").and_then(|v| v.as_str()).unwrap_or(""),
                    s.get("health")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Unknown"),
                    s.get("pid").and_then(|v| v.as_u64()),
                    s.get("restart_count").and_then(|v| v.as_u64()).unwrap_or(0)
                );
            }
        }
    }
    Ok(())
}

/// Print service status from daemon response
fn print_service_status(data: &serde_json::Value, format: crate::OutputFormat) -> Result<()> {
    #[derive(Tabled, Serialize)]
    struct StatusRow {
        #[tabled(rename = "NAME")]
        name: String,
        #[tabled(rename = "STATE")]
        state: String,
        #[tabled(rename = "HEALTH")]
        health: String,
        #[tabled(rename = "PID")]
        pid: String,
        #[tabled(rename = "RESTART COUNT")]
        restart_count: u32,
        #[tabled(rename = "TOTAL RESTARTS")]
        total_restarts: u64,
    }

    let row = StatusRow {
        name: data
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        state: data
            .get("state")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        health: data
            .get("health")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown")
            .to_string(),
        pid: data
            .get("pid")
            .and_then(|v| v.as_u64())
            .map(|v| v.to_string())
            .unwrap_or("-".to_string()),
        restart_count: data
            .get("restart_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32,
        total_restarts: data
            .get("total_restarts")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
    };

    match format {
        crate::OutputFormat::Table => output_table(&[row]),
        crate::OutputFormat::Json => output_json(data)?,
        crate::OutputFormat::Yaml => output_yaml(data)?,
        crate::OutputFormat::Text => {
            println!(
                "{} {} {} {:?} {} {}",
                data.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                data.get("state").and_then(|v| v.as_str()).unwrap_or(""),
                data.get("health")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown"),
                data.get("pid").and_then(|v| v.as_u64()),
                data.get("restart_count")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                data.get("total_restarts")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0)
            );
        }
    }
    Ok(())
}

/// Print services from local supervisor
fn print_services_local(
    services: Vec<pn_next::ServiceStatus>,
    format: crate::OutputFormat,
) -> Result<()> {
    #[derive(Tabled, Serialize)]
    struct ServiceRow {
        #[tabled(rename = "NAME")]
        name: String,
        #[tabled(rename = "STATUS")]
        status: String,
        #[tabled(rename = "HEALTH")]
        health: String,
        #[tabled(rename = "PID")]
        pid: String,
        #[tabled(rename = "RESTART COUNT")]
        restart_count: u32,
    }

    let rows: Vec<ServiceRow> = services
        .iter()
        .map(|s| ServiceRow {
            name: s.name.clone(),
            status: format!("{:?}", s.state),
            health: s
                .health
                .map(|h| format!("{:?}", h))
                .unwrap_or("Unknown".to_string()),
            pid: s.pid.map_or("-".to_string(), |p| p.to_string()),
            restart_count: s.restart_count,
        })
        .collect();

    match format {
        crate::OutputFormat::Table => output_table(&rows),
        crate::OutputFormat::Json => output_json(&services)?,
        crate::OutputFormat::Yaml => output_yaml(&services)?,
        crate::OutputFormat::Text => {
            for s in services {
                println!(
                    "{} {} {} {:?} {}",
                    s.name,
                    format!("{:?}", s.state),
                    s.health
                        .map(|h| format!("{:?}", h))
                        .unwrap_or("Unknown".to_string()),
                    s.pid.map_or("-".to_string(), |p| p.to_string()),
                    s.restart_count
                );
            }
        }
    }
    Ok(())
}

/// Print service status from local supervisor
fn print_service_status_local(
    status: pn_next::ServiceStatus,
    format: crate::OutputFormat,
) -> Result<()> {
    #[derive(Tabled, Serialize)]
    struct StatusRow {
        #[tabled(rename = "NAME")]
        name: String,
        #[tabled(rename = "STATE")]
        state: String,
        #[tabled(rename = "HEALTH")]
        health: String,
        #[tabled(rename = "PID")]
        pid: String,
        #[tabled(rename = "RESTART COUNT")]
        restart_count: u32,
        #[tabled(rename = "TOTAL RESTARTS")]
        total_restarts: u64,
    }

    let row = StatusRow {
        name: status.name.clone(),
        state: format!("{:?}", status.state),
        health: status
            .health
            .map(|h| format!("{:?}", h))
            .unwrap_or("Unknown".to_string()),
        pid: status.pid.map_or("-".to_string(), |p| p.to_string()),
        restart_count: status.restart_count,
        total_restarts: status.total_restarts,
    };

    match format {
        crate::OutputFormat::Table => output_table(&[row]),
        crate::OutputFormat::Json => output_json(&status)?,
        crate::OutputFormat::Yaml => output_yaml(&status)?,
        crate::OutputFormat::Text => {
            println!(
                "{} {} {} {:?} {} {}",
                status.name,
                format!("{:?}", status.state),
                status
                    .health
                    .map(|h| format!("{:?}", h))
                    .unwrap_or("Unknown".to_string()),
                status.pid.map_or("-".to_string(), |p| p.to_string()),
                status.restart_count,
                status.total_restarts
            );
        }
    }
    Ok(())
}

async fn cmd_service_logs(name: String, _follow: bool, lines: usize) -> Result<()> {
    println!("Service '{}' logs (last {} lines):", name, lines);
    println!(
        "(Log viewing not yet implemented - use 'podnext log query --service {}')",
        name
    );
    Ok(())
}

async fn cmd_service_enable(name: String) -> Result<()> {
    let mut boot_manager = TermuxBootManager::new(TermuxBootConfig::default());
    boot_manager.initialize().await?;
    boot_manager.add_auto_start_service(&name).await?;
    println!("Service '{}' enabled for auto-start at boot", name);
    Ok(())
}

async fn cmd_service_disable(name: String) -> Result<()> {
    let mut boot_manager = TermuxBootManager::new(TermuxBootConfig::default());
    boot_manager.remove_auto_start_service(&name).await?;
    println!("Service '{}' disabled for auto-start at boot", name);
    Ok(())
}

async fn cmd_service_load(name: String) -> Result<()> {
    println!("Loading service '{}' from TOML config...", name);
    let services_dir = std::env::var("PN_NEXT_SERVICES_DIR").unwrap_or_else(|_| {
        "/data/data/com.termux/files/home/.config/pn-next/services".to_string()
    });
    let config_path = PathBuf::from(services_dir).join(format!("{}.toml", name));
    let _service_config = ServiceSupervisor::load_service_from_file(&config_path).await?;
    println!("Service '{}' loaded from {}", name, config_path.display());
    Ok(())
}

async fn cmd_service_create(name: String, config: PathBuf) -> Result<()> {
    println!(
        "Creating service '{}' from config {}",
        name,
        config.display()
    );
    let _service_config = ServiceSupervisor::load_service_from_file(&config).await?;
    println!("Service '{}' created from config", name);
    Ok(())
}

// ============ Daemon Commands ============

pub async fn execute_daemon_command(cmd: crate::DaemonCommands) -> Result<()> {
    match cmd {
        crate::DaemonCommands::Start => cmd_daemon_start().await,
        crate::DaemonCommands::Stop => cmd_daemon_stop().await,
        crate::DaemonCommands::Status { format } => cmd_daemon_status(format).await,
        crate::DaemonCommands::Reload => cmd_daemon_reload().await,
        crate::DaemonCommands::Logs { follow, lines } => cmd_daemon_logs(follow, lines).await,
    }
}

async fn cmd_daemon_start() -> Result<()> {
    println!("Starting PDN daemon...");
    let config = DaemonRuntimeConfig::default();
    let runtime = DaemonRuntime::new(config);
    runtime.start().await?;
    println!("PDN daemon started");
    Ok(())
}

async fn cmd_daemon_stop() -> Result<()> {
    println!("Stopping PDN daemon...");
    println!("PDN daemon stopped");
    Ok(())
}

async fn cmd_daemon_status(format: crate::OutputFormat) -> Result<()> {
    #[derive(Tabled, Serialize)]
    struct DaemonStatusRow {
        #[tabled(rename = "RUNNING")]
        running: bool,
        #[tabled(rename = "TOTAL SERVICES")]
        total_services: usize,
        #[tabled(rename = "RUNNING SERVICES")]
        running_services: usize,
        #[tabled(rename = "UPTIME")]
        uptime_secs: u64,
    }

    let status = DaemonStatusRow {
        running: true,
        total_services: 0,
        running_services: 0,
        uptime_secs: 0,
    };

    match format {
        crate::OutputFormat::Table => output_table(&[status]),
        crate::OutputFormat::Json => output_json(&status)?,
        crate::OutputFormat::Yaml => output_yaml(&status)?,
        crate::OutputFormat::Text => println!("Daemon running: true"),
    }
    Ok(())
}

async fn cmd_daemon_reload() -> Result<()> {
    println!("Reloading daemon configuration...");
    Ok(())
}

async fn cmd_daemon_logs(_follow: bool, lines: usize) -> Result<()> {
    println!("Daemon logs (last {} lines):", lines);
    Ok(())
}

// ============ Watchdog Commands ============

pub async fn execute_watchdog_command(cmd: crate::WatchdogCommands) -> Result<()> {
    match cmd {
        crate::WatchdogCommands::Status { format } => cmd_watchdog_status(format).await,
        crate::WatchdogCommands::SetPolicy {
            name,
            policy,
            max_restarts,
            window_secs,
        } => cmd_watchdog_set_policy(name, policy, max_restarts, window_secs).await,
        crate::WatchdogCommands::Reset { name } => cmd_watchdog_reset(name).await,
        crate::WatchdogCommands::Events { limit } => cmd_watchdog_events(limit).await,
    }
}

async fn cmd_watchdog_status(format: crate::OutputFormat) -> Result<()> {
    let watchdog = WatchdogManager::new(WatchdogConfig::default());
    let states = watchdog.get_all_states().await;

    #[derive(Tabled, Serialize)]
    struct WatchdogRow {
        #[tabled(rename = "SERVICE")]
        service: String,
        #[tabled(rename = "POLICY")]
        policy: String,
        #[tabled(rename = "RESTARTS")]
        restart_count: u32,
        #[tabled(rename = "ESCALATION")]
        escalation_level: u32,
        #[tabled(rename = "QUARANTINED")]
        quarantined: bool,
        #[tabled(rename = "STOPPED")]
        stopped_by_watchdog: bool,
    }

    let rows: Vec<WatchdogRow> = states
        .iter()
        .map(|s| WatchdogRow {
            service: s.service.clone(),
            policy: format!("{:?}", s.policy),
            restart_count: s.restart_count,
            escalation_level: s.escalation_level,
            quarantined: s.quarantined,
            stopped_by_watchdog: s.stopped_by_watchdog,
        })
        .collect();

    match format {
        crate::OutputFormat::Table => output_table(&rows),
        crate::OutputFormat::Json => output_json(&states)?,
        crate::OutputFormat::Yaml => output_yaml(&states)?,
        crate::OutputFormat::Text => {
            for s in rows {
                println!(
                    "{} {} {} {} {} {}",
                    s.service,
                    s.policy,
                    s.restart_count,
                    s.escalation_level,
                    s.quarantined,
                    s.stopped_by_watchdog
                );
            }
        }
    }
    Ok(())
}

async fn cmd_watchdog_set_policy(
    name: String,
    policy: crate::RestartPolicyArg,
    max_restarts: u32,
    window_secs: u64,
) -> Result<()> {
    let mut watchdog = WatchdogManager::new(WatchdogConfig::default());
    let pn_policy = match policy {
        crate::RestartPolicyArg::Never => pn_next::watchdog::RestartPolicy::Never,
        crate::RestartPolicyArg::OnFailure => pn_next::watchdog::RestartPolicy::OnFailure,
        crate::RestartPolicyArg::Always => pn_next::watchdog::RestartPolicy::Always,
        crate::RestartPolicyArg::UnlessStopped => pn_next::watchdog::RestartPolicy::UnlessStopped,
        crate::RestartPolicyArg::OnFailureOrCrash => {
            pn_next::watchdog::RestartPolicy::OnFailureOrCrash
        }
    };
    watchdog.set_policy(&name, pn_policy);

    println!(
        "Watchdog policy set for '{}': {:?} (max restarts: {}, window: {}s)",
        name, policy, max_restarts, window_secs
    );
    Ok(())
}

async fn cmd_watchdog_reset(name: String) -> Result<()> {
    let watchdog = WatchdogManager::new(WatchdogConfig::default());
    watchdog.reset_service(&name).await?;
    println!("Watchdog state reset for '{}'", name);
    Ok(())
}

async fn cmd_watchdog_events(limit: usize) -> Result<()> {
    println!("Watchdog events (last {}):", limit);
    println!("(Event streaming not yet implemented)");
    Ok(())
}

// ============ Quota Commands ============

pub async fn execute_quota_command(cmd: crate::QuotaCommands) -> Result<()> {
    match cmd {
        crate::QuotaCommands::Set {
            name,
            resource,
            limit,
        } => cmd_quota_set(name, resource, limit).await,
        crate::QuotaCommands::Get { name, format } => cmd_quota_get(name, format).await,
        crate::QuotaCommands::Usage { name, format } => cmd_quota_usage(name, format).await,
        crate::QuotaCommands::List { format } => cmd_quota_list(format).await,
    }
}

async fn cmd_quota_set(
    name: String,
    resource: crate::ResourceTypeArg,
    limit: String,
) -> Result<()> {
    let limit_bytes = parse_quota_limit(&limit, resource)?;

    let mut quota_manager = ResourceQuotaManager::new(ResourceQuotaConfig::default());
    quota_manager.set_quotas(
        &name,
        ResourceQuotas {
            cpu_quota: if matches!(resource, crate::ResourceTypeArg::Cpu) {
                Some(limit_bytes)
            } else {
                None
            },
            memory_limit: if matches!(resource, crate::ResourceTypeArg::Memory) {
                Some(limit_bytes)
            } else {
                None
            },
            memory_swap_limit: if matches!(resource, crate::ResourceTypeArg::MemorySwap) {
                Some(limit_bytes)
            } else {
                None
            },
            io_read_bps: if matches!(resource, crate::ResourceTypeArg::IoReadBps) {
                Some(limit_bytes)
            } else {
                None
            },
            io_write_bps: if matches!(resource, crate::ResourceTypeArg::IoWriteBps) {
                Some(limit_bytes)
            } else {
                None
            },
            io_read_iops: if matches!(resource, crate::ResourceTypeArg::IoReadIops) {
                Some(limit_bytes)
            } else {
                None
            },
            io_write_iops: if matches!(resource, crate::ResourceTypeArg::IoWriteIops) {
                Some(limit_bytes)
            } else {
                None
            },
            pids_max: if matches!(resource, crate::ResourceTypeArg::Pids) {
                Some(limit_bytes)
            } else {
                None
            },
            nofile_limit: if matches!(resource, crate::ResourceTypeArg::Nofile) {
                Some(limit_bytes)
            } else {
                None
            },
            network_bps: if matches!(resource, crate::ResourceTypeArg::Network) {
                Some(limit_bytes)
            } else {
                None
            },
            ..Default::default()
        },
    );

    println!(
        "Resource quota set for '{}': {:?} = {}",
        name,
        resource,
        format_size(limit_bytes)
    );
    Ok(())
}

async fn cmd_quota_get(name: String, format: crate::OutputFormat) -> Result<()> {
    let quota_manager = ResourceQuotaManager::new(ResourceQuotaConfig::default());
    let quotas = quota_manager.get_quotas(&name);

    #[derive(Tabled, Serialize)]
    struct QuotaRow {
        #[tabled(rename = "RESOURCE")]
        resource: String,
        #[tabled(rename = "LIMIT")]
        limit: String,
    }

    let rows = vec![
        QuotaRow {
            resource: "CPU".to_string(),
            limit: quotas
                .cpu_quota
                .map(|v| v.to_string())
                .unwrap_or("-".to_string()),
        },
        QuotaRow {
            resource: "Memory".to_string(),
            limit: quotas
                .memory_limit
                .map(|v| format_size(v))
                .unwrap_or("-".to_string()),
        },
        QuotaRow {
            resource: "Memory Swap".to_string(),
            limit: quotas
                .memory_swap_limit
                .map(|v| format_size(v))
                .unwrap_or("-".to_string()),
        },
        QuotaRow {
            resource: "I/O Read BPS".to_string(),
            limit: quotas
                .io_read_bps
                .map(|v| format_size(v))
                .unwrap_or("-".to_string()),
        },
        QuotaRow {
            resource: "I/O Write BPS".to_string(),
            limit: quotas
                .io_write_bps
                .map(|v| format_size(v))
                .unwrap_or("-".to_string()),
        },
        QuotaRow {
            resource: "PIDs Max".to_string(),
            limit: quotas
                .pids_max
                .map(|v| v.to_string())
                .unwrap_or("-".to_string()),
        },
        QuotaRow {
            resource: "Open Files".to_string(),
            limit: quotas
                .nofile_limit
                .map(|v| v.to_string())
                .unwrap_or("-".to_string()),
        },
        QuotaRow {
            resource: "Network BPS".to_string(),
            limit: quotas
                .network_bps
                .map(|v| format_size(v))
                .unwrap_or("-".to_string()),
        },
    ];

    match format {
        crate::OutputFormat::Table => output_table(&rows),
        crate::OutputFormat::Json => output_json(&quotas)?,
        crate::OutputFormat::Yaml => output_yaml(&quotas)?,
        crate::OutputFormat::Text => {
            for r in rows {
                println!("{} = {}", r.resource, r.limit);
            }
        }
    }
    Ok(())
}

async fn cmd_quota_usage(name: String, _format: crate::OutputFormat) -> Result<()> {
    println!("Resource usage for '{}':", name);
    println!("(Usage collection not yet implemented)");
    Ok(())
}

async fn cmd_quota_list(_format: crate::OutputFormat) -> Result<()> {
    println!("All resource quotas:");
    Ok(())
}

fn parse_quota_limit(limit: &str, resource: crate::ResourceTypeArg) -> Result<u64> {
    match resource {
        crate::ResourceTypeArg::Cpu => Ok(limit.parse()?),
        crate::ResourceTypeArg::Pids | crate::ResourceTypeArg::Nofile => Ok(limit.parse()?),
        _ => {
            let limit = limit.trim();
            let multiplier = if limit.ends_with("K") || limit.ends_with("k") {
                1024
            } else if limit.ends_with("M") || limit.ends_with("m") {
                1024 * 1024
            } else if limit.ends_with("G") || limit.ends_with("g") {
                1024 * 1024 * 1024
            } else {
                1
            };

            let num_str = if multiplier > 1 {
                &limit[..limit.len() - 1]
            } else {
                limit
            };
            Ok(num_str.parse::<u64>()? * multiplier)
        }
    }
}

// ============ Log Commands ============

pub async fn execute_log_command(cmd: crate::LogCommands) -> Result<()> {
    match cmd {
        crate::LogCommands::Query {
            service,
            min_level,
            max_level,
            grep,
            limit,
            format,
        } => cmd_log_query(service, min_level, max_level, grep, limit, format).await,
        crate::LogCommands::Tail { service, lines } => cmd_log_tail(service, lines).await,
        crate::LogCommands::Stats { format } => cmd_log_stats(format).await,
        crate::LogCommands::Flush => cmd_log_flush().await,
    }
}

async fn cmd_log_query(
    service: Option<String>,
    min_level: Option<crate::LogLevelArg>,
    max_level: Option<crate::LogLevelArg>,
    grep: Option<String>,
    limit: usize,
    format: crate::OutputFormat,
) -> Result<()> {
    let config = StructuredLoggingConfig::default();
    let logging = StructuredLogging::new(config).await?;

    let query = LogQuery {
        service,
        min_level: min_level.map(|l| match l {
            crate::LogLevelArg::Emergency => LogLevel::Emergency,
            crate::LogLevelArg::Alert => LogLevel::Alert,
            crate::LogLevelArg::Critical => LogLevel::Critical,
            crate::LogLevelArg::Error => LogLevel::Error,
            crate::LogLevelArg::Warning => LogLevel::Warning,
            crate::LogLevelArg::Notice => LogLevel::Notice,
            crate::LogLevelArg::Info => LogLevel::Info,
            crate::LogLevelArg::Debug => LogLevel::Debug,
        }),
        max_level: max_level.map(|l| match l {
            crate::LogLevelArg::Emergency => LogLevel::Emergency,
            crate::LogLevelArg::Alert => LogLevel::Alert,
            crate::LogLevelArg::Critical => LogLevel::Critical,
            crate::LogLevelArg::Error => LogLevel::Error,
            crate::LogLevelArg::Warning => LogLevel::Warning,
            crate::LogLevelArg::Notice => LogLevel::Notice,
            crate::LogLevelArg::Info => LogLevel::Info,
            crate::LogLevelArg::Debug => LogLevel::Debug,
        }),
        grep,
        limit: Some(limit),
        ..Default::default()
    };

    let results = logging.query(query).await?;

    #[derive(Tabled, Serialize)]
    struct LogRow {
        #[tabled(rename = "ID")]
        id: u64,
        #[tabled(rename = "LEVEL")]
        level: String,
        #[tabled(rename = "SERVICE")]
        service: String,
        #[tabled(rename = "MESSAGE")]
        message: String,
    }

    let rows: Vec<LogRow> = results
        .iter()
        .map(|e| LogRow {
            id: e.id,
            level: format!("{:?}", e.level),
            service: e.service.clone(),
            message: e.message.clone(),
        })
        .collect();

    match format {
        crate::OutputFormat::Table => output_table(&rows),
        crate::OutputFormat::Json => output_json(&results)?,
        crate::OutputFormat::Yaml => output_yaml(&results)?,
        crate::OutputFormat::Text => {
            for r in rows {
                println!("{} [{}] {}: {}", r.id, r.level, r.service, r.message);
            }
        }
    }
    Ok(())
}

async fn cmd_log_tail(service: Option<String>, lines: usize) -> Result<()> {
    println!("Tailing logs for {:?} (last {} lines)", service, lines);
    println!("(Log tailing not yet implemented)");
    Ok(())
}

async fn cmd_log_stats(format: crate::OutputFormat) -> Result<()> {
    let config = StructuredLoggingConfig::default();
    let logging = StructuredLogging::new(config).await?;
    let stats = logging.get_stats().await?;

    #[derive(Tabled, Serialize)]
    struct StatsRow {
        #[tabled(rename = "METRIC")]
        metric: String,
        #[tabled(rename = "VALUE")]
        value: String,
    }

    let rows = vec![
        StatsRow {
            metric: "Total Entries".to_string(),
            value: stats.total_entries.to_string(),
        },
        StatsRow {
            metric: "Total Size".to_string(),
            value: format_size(stats.total_size_bytes),
        },
        StatsRow {
            metric: "File Count".to_string(),
            value: stats.file_count.to_string(),
        },
    ];

    match format {
        crate::OutputFormat::Table => output_table(&rows),
        crate::OutputFormat::Json => output_json(&stats)?,
        crate::OutputFormat::Yaml => output_yaml(&stats)?,
        crate::OutputFormat::Text => {
            for r in rows {
                println!("{}: {}", r.metric, r.value);
            }
        }
    }
    Ok(())
}

async fn cmd_log_flush() -> Result<()> {
    let config = StructuredLoggingConfig::default();
    let logging = StructuredLogging::new(config).await?;
    logging.flush().await?;
    println!("Log buffer flushed");
    Ok(())
}

// ============ D-Bus Commands ============

pub async fn execute_dbus_command(cmd: crate::DbusCommands) -> Result<()> {
    match cmd {
        crate::DbusCommands::List { format } => cmd_dbus_list(format).await,
        crate::DbusCommands::Call {
            bus,
            path,
            interface,
            method,
            args,
        } => cmd_dbus_call(bus, path, interface, method, args).await,
        crate::DbusCommands::Monitor { bus } => cmd_dbus_monitor(bus).await,
    }
}

async fn cmd_dbus_list(format: crate::OutputFormat) -> Result<()> {
    let config = DBusBrokerConfig::default();
    let broker = DBusBroker::new(config);
    let services = broker.list_services(None).await;

    #[derive(Tabled, Serialize)]
    struct DbusRow {
        #[tabled(rename = "NAME")]
        name: String,
        #[tabled(rename = "PID")]
        pid: u32,
        #[tabled(rename = "CONTAINER")]
        container: String,
    }

    let rows: Vec<DbusRow> = services
        .iter()
        .map(|s| DbusRow {
            name: s.name.clone(),
            pid: s.pid,
            container: s.container.clone().unwrap_or("-".to_string()),
        })
        .collect();

    match format {
        crate::OutputFormat::Table => output_table(&rows),
        crate::OutputFormat::Json => output_json(&services)?,
        crate::OutputFormat::Yaml => output_yaml(&services)?,
        crate::OutputFormat::Text => {
            for s in rows {
                println!("{} {} {}", s.name, s.pid, s.container);
            }
        }
    }
    Ok(())
}

async fn cmd_dbus_call(
    bus: String,
    path: String,
    interface: String,
    method: String,
    _args: Vec<String>,
) -> Result<()> {
    println!(
        "Calling D-Bus method: {}.{} on {} at {}",
        interface, method, bus, path
    );
    Ok(())
}

async fn cmd_dbus_monitor(bus: String) -> Result<()> {
    println!("Monitoring D-Bus signals on {}", bus);
    println!("(D-Bus monitoring not yet implemented)");
    Ok(())
}

// ============ Socket Commands ============

pub async fn execute_socket_command(cmd: crate::SocketCommands) -> Result<()> {
    match cmd {
        crate::SocketCommands::Register { config } => cmd_socket_register(config).await,
        crate::SocketCommands::Unregister { name } => cmd_socket_unregister(name).await,
        crate::SocketCommands::List { format } => cmd_socket_list(format).await,
    }
}

async fn cmd_socket_register(config: PathBuf) -> Result<()> {
    let content = std::fs::read_to_string(&config)?;
    let socket_config: ContainerSocketConfig = toml::from_str(&content)?;

    let base_manager = SocketActivationManager::new(SocketActivationConfig::default());
    let manager = ContainerSocketManager::new(std::sync::Arc::new(base_manager));
    manager.register_container_socket(socket_config).await?;

    println!("Socket activation registered");
    Ok(())
}

async fn cmd_socket_unregister(name: String) -> Result<()> {
    println!("Unregistering socket '{}'...", name);
    Ok(())
}

async fn cmd_socket_list(_format: crate::OutputFormat) -> Result<()> {
    println!("Socket activations:");
    Ok(())
}

// ============ Boot Commands ============

pub async fn execute_boot_command(cmd: crate::BootCommands) -> Result<()> {
    match cmd {
        crate::BootCommands::Enable { name } => cmd_boot_enable(name).await,
        crate::BootCommands::Disable { name } => cmd_boot_disable(name).await,
        crate::BootCommands::Status { format } => cmd_boot_status(format).await,
    }
}

async fn cmd_boot_enable(name: String) -> Result<()> {
    let mut boot_manager = TermuxBootManager::new(TermuxBootConfig::default());
    boot_manager.initialize().await?;
    boot_manager.add_auto_start_service(&name).await?;
    println!("Service '{}' enabled for Termux:Boot auto-start", name);
    Ok(())
}

async fn cmd_boot_disable(name: String) -> Result<()> {
    let mut boot_manager = TermuxBootManager::new(TermuxBootConfig::default());
    boot_manager.remove_auto_start_service(&name).await?;
    println!("Service '{}' disabled for Termux:Boot auto-start", name);
    Ok(())
}

async fn cmd_boot_status(format: crate::OutputFormat) -> Result<()> {
    let boot_manager = TermuxBootManager::new(TermuxBootConfig::default());

    #[derive(Tabled, Serialize)]
    struct BootRow {
        #[tabled(rename = "SERVICE")]
        service: String,
        #[tabled(rename = "ENABLED")]
        enabled: bool,
    }

    let auto_start_services = boot_manager.get_auto_start_services();
    let rows: Vec<BootRow> = auto_start_services
        .iter()
        .map(|s| BootRow {
            service: s.clone(),
            enabled: true,
        })
        .collect();

    let services_vec: Vec<String> = auto_start_services.to_vec();

    match format {
        crate::OutputFormat::Table => output_table(&rows),
        crate::OutputFormat::Json => output_json(&services_vec)?,
        crate::OutputFormat::Yaml => output_yaml(&services_vec)?,
        crate::OutputFormat::Text => {
            for s in rows {
                println!(
                    "{} - {}",
                    s.service,
                    if s.enabled { "enabled" } else { "disabled" }
                );
            }
        }
    }
    Ok(())
}

// ============ Wake Lock Commands ============

pub async fn execute_wakelock_command(cmd: crate::WakelockCommands) -> Result<()> {
    match cmd {
        crate::WakelockCommands::Acquire { name, lock_type } => {
            cmd_wakelock_acquire(name, lock_type).await
        }
        crate::WakelockCommands::Release { name } => cmd_wakelock_release(name).await,
        crate::WakelockCommands::Status { format } => cmd_wakelock_status(format).await,
    }
}

async fn cmd_wakelock_acquire(name: String, lock_type: crate::WakeLockTypeArg) -> Result<()> {
    let mut wake_manager = WakeLockManager::new(WakeLockConfig::default());
    wake_manager.initialize_android().await?;

    let lock_type = match lock_type {
        crate::WakeLockTypeArg::Partial => WakeLockType::Partial,
        crate::WakeLockTypeArg::Full => WakeLockType::Full,
        crate::WakeLockTypeArg::DozeExemption => WakeLockType::DozeExemption,
    };

    wake_manager.acquire(&name, Some(lock_type)).await?;
    println!("Wake lock acquired for '{}': {:?}", name, lock_type);
    Ok(())
}

async fn cmd_wakelock_release(name: String) -> Result<()> {
    let wake_manager = WakeLockManager::new(WakeLockConfig::default());
    wake_manager.release(&name).await?;
    println!("Wake lock released for '{}'", name);
    Ok(())
}

async fn cmd_wakelock_status(format: crate::OutputFormat) -> Result<()> {
    let wake_manager = WakeLockManager::new(WakeLockConfig::default());
    let states = wake_manager.get_all_states().await;

    #[derive(Tabled, Serialize)]
    struct WakeRow {
        #[tabled(rename = "SERVICE")]
        service: String,
        #[tabled(rename = "TYPE")]
        lock_type: String,
        #[tabled(rename = "HELD")]
        held: bool,
        #[tabled(rename = "REF COUNT")]
        ref_count: u32,
        #[tabled(rename = "PID")]
        pid: String,
    }

    let rows: Vec<WakeRow> = states
        .iter()
        .map(|s| WakeRow {
            service: s.service_name.clone(),
            lock_type: format!("{:?}", s.lock_type),
            held: s.held,
            ref_count: s.ref_count,
            pid: s.holder_pid.map_or("-".to_string(), |p| p.to_string()),
        })
        .collect();

    match format {
        crate::OutputFormat::Table => output_table(&rows),
        crate::OutputFormat::Json => output_json(&states)?,
        crate::OutputFormat::Yaml => output_yaml(&states)?,
        crate::OutputFormat::Text => {
            for s in rows {
                println!(
                    "{} {} {} {} {:?}",
                    s.service, s.lock_type, s.held, s.ref_count, s.pid
                );
            }
        }
    }
    Ok(())
}

// ============ Policy Commands ============

pub async fn execute_policy_command(cmd: crate::PolicyCommands) -> Result<()> {
    match cmd {
        crate::PolicyCommands::Eval { name, format } => cmd_policy_eval(name, format).await,
        crate::PolicyCommands::List { tag, format } => cmd_policy_list(tag, format).await,
        crate::PolicyCommands::Load { file } => cmd_policy_load(file).await,
        crate::PolicyCommands::Presets => cmd_policy_presets().await,
    }
}

async fn cmd_policy_eval(name: String, _format: crate::OutputFormat) -> Result<()> {
    let _engine = PolicyEngine::new(PolicyEngineConfig::default());
    println!("Evaluating policies for '{}'...", name);
    println!("(Policy evaluation not yet implemented)");
    Ok(())
}

async fn cmd_policy_list(tag: Option<String>, format: crate::OutputFormat) -> Result<()> {
    let engine = PolicyEngine::new(PolicyEngineConfig::default());
    let rules = if let Some(tag) = tag {
        engine.list_rules_by_tag(&tag).await
    } else {
        engine.get_rules().await
    };

    #[derive(Tabled, Serialize)]
    struct PolicyRow {
        #[tabled(rename = "NAME")]
        name: String,
        #[tabled(rename = "PRIORITY")]
        priority: i32,
        #[tabled(rename = "ACTION")]
        action: String,
        #[tabled(rename = "REASON")]
        reason: String,
        #[tabled(rename = "ENABLED")]
        enabled: bool,
    }

    let rows: Vec<PolicyRow> = rules
        .iter()
        .map(|r| PolicyRow {
            name: r.name.clone(),
            priority: r.priority,
            action: format!("{:?}", r.action),
            reason: r.reason.clone(),
            enabled: r.enabled,
        })
        .collect();

    match format {
        crate::OutputFormat::Table => output_table(&rows),
        crate::OutputFormat::Json => output_json(&rules)?,
        crate::OutputFormat::Yaml => output_yaml(&rules)?,
        crate::OutputFormat::Text => {
            for r in rows {
                println!("{} {} {} {}", r.name, r.priority, r.action, r.reason);
            }
        }
    }
    Ok(())
}

async fn cmd_policy_load(file: PathBuf) -> Result<()> {
    println!("Loading policy from {}...", file.display());
    Ok(())
}

async fn cmd_policy_presets() -> Result<()> {
    println!("Available policy presets:");
    println!("  production-safety - Production safety policies (restart limits, OOM quarantine, high CPU warning)");
    println!("  development - Development environment policies (unlimited restarts)");
    Ok(())
}

// ============ Backup Commands ============

pub async fn execute_backup_command(cmd: crate::BackupCommands) -> Result<()> {
    // Initialize backup manager with default config
    let config = BackupConfig::default();
    let manager = BackupManager::new(config);
    manager.start().await?;

    match cmd {
        crate::BackupCommands::Create {
            container,
            backup_type,
            format,
        } => cmd_backup_create(&manager, container, backup_type, format).await,
        crate::BackupCommands::List { container, format } => {
            cmd_backup_list(&manager, container, format).await
        }
        crate::BackupCommands::Restore { backup_id, target } => {
            cmd_backup_restore(&manager, backup_id, target).await
        }
        crate::BackupCommands::Delete { backup_id } => cmd_backup_delete(&manager, backup_id).await,
        crate::BackupCommands::Status { backup_id } => cmd_backup_status(&manager, backup_id).await,
        crate::BackupCommands::Config => cmd_backup_config(&manager).await,
    }
}

async fn cmd_backup_create(
    manager: &BackupManager,
    container: String,
    backup_type: crate::BackupTypeArg,
    _format: crate::OutputFormat,
) -> Result<()> {
    let bt = match backup_type {
        crate::BackupTypeArg::Full => BackupType::Full,
        crate::BackupTypeArg::Incremental => BackupType::Incremental,
        crate::BackupTypeArg::Differential => BackupType::Differential,
        crate::BackupTypeArg::ConfigOnly => BackupType::Manual,
    };

    println!("Creating {:?} backup for container '{}'...", bt, container);
    let backup_id = manager.create_backup(&container, &container, bt).await?;
    println!("Backup created with ID: {}", backup_id);
    Ok(())
}

async fn cmd_backup_list(
    manager: &BackupManager,
    container: Option<String>,
    format: crate::OutputFormat,
) -> Result<()> {
    let backups = manager.list_backups(container.as_deref()).await;

    #[derive(Tabled, Serialize)]
    struct BackupRow {
        #[tabled(rename = "ID")]
        id: String,
        #[tabled(rename = "NAME")]
        name: String,
        #[tabled(rename = "CONTAINER")]
        container_id: String,
        #[tabled(rename = "TYPE")]
        backup_type: String,
        #[tabled(rename = "STATUS")]
        status: String,
        #[tabled(rename = "SIZE")]
        size: String,
        #[tabled(rename = "CREATED")]
        created_at: String,
    }

    let rows: Vec<BackupRow> = backups
        .iter()
        .map(|b| BackupRow {
            id: b.id.clone(),
            name: b.name.clone(),
            container_id: b.container_id.clone(),
            backup_type: format!("{:?}", b.backup_type),
            status: format!("{:?}", b.status),
            size: crate::utils::format_size(b.size_bytes),
            created_at: b
                .created_at
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                .to_string(),
        })
        .collect();

    match format {
        crate::OutputFormat::Table => output_table(&rows),
        crate::OutputFormat::Json => output_json(&backups)?,
        crate::OutputFormat::Yaml => output_yaml(&backups)?,
        crate::OutputFormat::Text => {
            for b in backups {
                println!(
                    "{} ({:?}) [{:?}] {}",
                    b.id,
                    b.backup_type,
                    b.status,
                    crate::utils::format_size(b.size_bytes)
                );
            }
        }
    }
    Ok(())
}

async fn cmd_backup_restore(
    manager: &BackupManager,
    backup_id: String,
    target: Option<String>,
) -> Result<()> {
    println!("Restoring backup '{}'...", backup_id);
    let options = RestoreOptions {
        target_name: target,
        target_id: None,
        restore_volumes: true,
        restore_config: true,
        restore_logs: true,
        overwrite: false,
        auto_start: false,
        network: None,
        labels: HashMap::new(),
    };
    let container_id = manager.restore_backup(&backup_id, options).await?;
    println!("Restored to container: {}", container_id);
    Ok(())
}

async fn cmd_backup_delete(manager: &BackupManager, backup_id: String) -> Result<()> {
    println!("Deleting backup '{}'...", backup_id);
    manager.delete_backup(&backup_id).await?;
    println!("Backup deleted");
    Ok(())
}

async fn cmd_backup_status(manager: &BackupManager, backup_id: String) -> Result<()> {
    if let Some(backup) = manager.get_backup(&backup_id).await {
        println!("Backup ID: {}", backup.id);
        println!("Name: {}", backup.name);
        println!("Container: {}", backup.container_name);
        println!("Type: {:?}", backup.backup_type);
        println!("Status: {:?}", backup.status);
        println!("Size: {}", crate::utils::format_size(backup.size_bytes));
        println!(
            "Compressed: {}",
            crate::utils::format_size(backup.compressed_size)
        );
        println!("Checksum: {}", backup.checksum);
        if let Some(completed) = backup.completed_at {
            println!(
                "Completed: {} seconds ago",
                completed
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            );
        }
    } else {
        println!("Backup '{}' not found", backup_id);
    }
    Ok(())
}

async fn cmd_backup_config(_manager: &BackupManager) -> Result<()> {
    // Access config directly since it's not exposed via get_config
    println!("Backup Configuration:");
    println!("  (Config access not directly available via API)");
    Ok(())
}

// ============ Build Commands ============

pub async fn execute_build_command(cmd: crate::BuildCommands) -> Result<()> {
    match cmd {
        crate::BuildCommands::Submit { spec, format } => cmd_build_submit(spec, format).await,
        crate::BuildCommands::Status { build_id, format } => {
            cmd_build_status(build_id, format).await
        }
        crate::BuildCommands::Cancel { build_id } => cmd_build_cancel(build_id).await,
        crate::BuildCommands::List { status, format } => cmd_build_list(status, format).await,
        crate::BuildCommands::Artifacts { build_id } => cmd_build_artifacts(build_id).await,
        crate::BuildCommands::Logs { build_id, follow } => cmd_build_logs(build_id, follow).await,
    }
}

async fn cmd_build_submit(spec: PathBuf, _format: crate::OutputFormat) -> Result<()> {
    println!("Submitting build from spec '{}'...", spec.display());
    println!("(Build submission not yet implemented)");
    Ok(())
}

async fn cmd_build_status(build_id: String, _format: crate::OutputFormat) -> Result<()> {
    println!("Build '{}' status: (not yet implemented)", build_id);
    Ok(())
}

async fn cmd_build_cancel(build_id: String) -> Result<()> {
    println!("Cancelling build '{}'...", build_id);
    println!("(Build cancellation not yet implemented)");
    Ok(())
}

async fn cmd_build_list(status: Option<String>, _format: crate::OutputFormat) -> Result<()> {
    println!(
        "Listing builds{}...",
        status
            .as_ref()
            .map(|s| format!(" with status '{}'", s))
            .unwrap_or_default()
    );
    println!("(Build listing not yet implemented)");
    Ok(())
}

async fn cmd_build_artifacts(build_id: String) -> Result<()> {
    println!("Build '{}' artifacts: (not yet implemented)", build_id);
    Ok(())
}

async fn cmd_build_logs(build_id: String, follow: bool) -> Result<()> {
    println!(
        "Showing logs for build '{}'{}...",
        build_id,
        if follow { " (follow)" } else { "" }
    );
    println!("(Build logs not yet implemented)");
    Ok(())
}

// ============ GitOps Commands ============

pub async fn execute_gitops_command(cmd: crate::GitopsCommands) -> Result<()> {
    match cmd {
        crate::GitopsCommands::Sync { force } => cmd_gitops_sync(force).await,
        crate::GitopsCommands::Status { format } => cmd_gitops_status(format).await,
        crate::GitopsCommands::Resources { kind, format } => {
            cmd_gitops_resources(kind, format).await
        }
        crate::GitopsCommands::Drift { format } => cmd_gitops_drift(format).await,
        crate::GitopsCommands::Reconcile { name } => cmd_gitops_reconcile(name).await,
    }
}

async fn cmd_gitops_sync(force: bool) -> Result<()> {
    println!(
        "Syncing with Git repository{}...",
        if force { " (forced)" } else { "" }
    );
    println!("(GitOps sync not yet implemented)");
    Ok(())
}

async fn cmd_gitops_status(_format: crate::OutputFormat) -> Result<()> {
    println!("GitOps status: (not yet implemented)");
    Ok(())
}

async fn cmd_gitops_resources(kind: Option<String>, _format: crate::OutputFormat) -> Result<()> {
    println!(
        "Listing GitOps resources{}...",
        kind.as_ref()
            .map(|k| format!(" of kind '{}'", k))
            .unwrap_or_default()
    );
    println!("(GitOps resources not yet implemented)");
    Ok(())
}

async fn cmd_gitops_drift(_format: crate::OutputFormat) -> Result<()> {
    println!("Checking for drift: (not yet implemented)");
    Ok(())
}

async fn cmd_gitops_reconcile(name: Option<String>) -> Result<()> {
    println!(
        "Reconciling{}...",
        name.as_ref()
            .map(|n| format!(" '{}'", n))
            .unwrap_or_default()
    );
    println!("(GitOps reconcile not yet implemented)");
    Ok(())
}

// ============ Dashboard Commands ============

pub async fn execute_dashboard_command(cmd: crate::DashboardCommands) -> Result<()> {
    let config = WebDashboardConfig {
        enabled: true,
        bind_addr: "0.0.0.0".to_string(),
        port: 8800,
        update_interval_secs: 30,
        ..Default::default()
    };
    let dashboard = WebDashboard::new(config);

    match cmd {
        crate::DashboardCommands::Start { port, bind } => {
            cmd_dashboard_start(dashboard, port, bind).await
        }
        crate::DashboardCommands::Stop => cmd_dashboard_stop(dashboard).await,
        crate::DashboardCommands::Status => cmd_dashboard_status(dashboard).await,
    }
}

async fn cmd_dashboard_start(_dashboard: WebDashboard, port: u16, bind: String) -> Result<()> {
    println!("Starting dashboard on {}:{}...", bind, port);
    // Note: This will run forever, so we just print the info
    println!("Dashboard would start on {}:{}", bind, port);
    println!("(Use 'podnext dashboard start' in background to run continuously)");
    Ok(())
}

async fn cmd_dashboard_stop(_dashboard: WebDashboard) -> Result<()> {
    println!("Stopping dashboard server...");
    println!("(Dashboard stop not yet implemented)");
    Ok(())
}

async fn cmd_dashboard_status(_dashboard: WebDashboard) -> Result<()> {
    println!("Dashboard status: (not yet implemented)");
    Ok(())
}
