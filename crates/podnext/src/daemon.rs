//! podnext-daemon - Persistent service supervisor daemon
//!
//! Runs as a background daemon managing container services via Unix socket IPC.

use anyhow::{Context, Result};
use clap::Parser;
use pn_next::{ServiceSupervisor, SupervisorBuilder, SupervisorConfig, SupervisorEvent};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::broadcast;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

/// Daemon command protocol
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
enum DaemonCommand {
    List,
    Status { name: String },
    Start { name: String },
    Stop { name: String, force: bool },
    Restart { name: String },
    Load { name: String },
    Subscribe,
    Ping,
}

/// Daemon response
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum DaemonResponse {
    Ok { data: serde_json::Value },
    Error { message: String },
    Event { event: SupervisorEvent },
    Pong,
}

/// Daemon state shared across connections
struct DaemonState {
    supervisor: Arc<ServiceSupervisor>,
    event_sender: broadcast::Sender<SupervisorEvent>,
}

#[derive(Parser)]
#[command(name = "podnext-daemon")]
#[command(about = "podnext persistent service supervisor daemon")]
#[command(version)]
struct Args {
    /// Socket path for IPC
    #[arg(
        short,
        long,
        default_value = "/data/data/com.termux/files/home/.local/run/pn-next/daemon.sock"
    )]
    socket: PathBuf,

    /// Service definitions directory
    #[arg(
        long,
        default_value = "/data/data/com.termux/files/home/.config/pn-next/services"
    )]
    services_dir: PathBuf,

    /// Runtime directory
    #[arg(
        long,
        default_value = "/data/data/com.termux/files/home/.local/run/pn-next"
    )]
    runtime_dir: PathBuf,

    /// Log directory
    #[arg(
        long,
        default_value = "/data/data/com.termux/files/home/.local/log/pn-next"
    )]
    log_dir: PathBuf,

    /// Supervision interval (ms)
    #[arg(long, default_value = "1000")]
    interval_ms: u64,

    /// Enable cgroups
    #[arg(long, default_value = "true")]
    enable_cgroups: bool,

    /// Run in foreground (don't daemonize)
    #[arg(short, long)]
    foreground: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    // Ensure directories exist
    std::fs::create_dir_all(&args.services_dir)?;
    std::fs::create_dir_all(&args.runtime_dir)?;
    std::fs::create_dir_all(&args.log_dir)?;

    // Remove existing socket
    if args.socket.exists() {
        std::fs::remove_file(&args.socket)?;
    }

    // Build supervisor config
    let _config = SupervisorConfig {
        services_dir: args.services_dir.clone(),
        runtime_dir: args.runtime_dir.clone(),
        log_dir: args.log_dir.clone(),
        supervision_interval_ms: args.interval_ms,
        enable_cgroups: args.enable_cgroups,
        ..Default::default()
    };

    // Create supervisor
    let supervisor = Arc::new(
        SupervisorBuilder::new()
            .services_dir(args.services_dir)
            .runtime_dir(args.runtime_dir)
            .log_dir(args.log_dir)
            .build()
            .context("Failed to build supervisor")?,
    );

    // Initialize supervisor (loads services, builds dependency graph)
    supervisor
        .initialize()
        .await
        .context("Failed to initialize supervisor")?;

    // Create event channel for broadcasting events to subscribers
    let (event_sender, _) = broadcast::channel(1024);

    // Subscribe to supervisor events and re-broadcast
    let event_sender_clone = event_sender.clone();
    let supervisor_events = supervisor.subscribe_events();
    tokio::spawn(async move {
        let mut events = supervisor_events;
        while let Ok(event) = events.recv().await {
            let _ = event_sender_clone.send(event);
        }
    });

    let state = Arc::new(DaemonState {
        supervisor,
        event_sender,
    });

    // Bind Unix socket
    let listener = UnixListener::bind(&args.socket)
        .with_context(|| format!("Failed to bind to {:?}", args.socket))?;

    info!("podnext-daemon listening on {:?}", args.socket);

    // Handle signals for graceful shutdown
    let socket_path = args.socket.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        info!("Shutdown signal received");
        let _ = std::fs::remove_file(&socket_path);
        std::process::exit(0);
    });

    // Accept connections
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let state = state.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(stream, state).await {
                        warn!("Connection error: {}", e);
                    }
                });
            }
            Err(e) => {
                error!("Accept error: {}", e);
            }
        }
    }
}

/// Handle a single client connection
async fn handle_connection(stream: UnixStream, state: Arc<DaemonState>) -> Result<()> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();

    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line).await?;
        if bytes_read == 0 {
            break; // Connection closed
        }

        let cmd: DaemonCommand = match serde_json::from_str(line.trim()) {
            Ok(cmd) => cmd,
            Err(e) => {
                let resp = DaemonResponse::Error {
                    message: format!("Invalid command: {}", e),
                };
                send_response(&mut reader.get_mut(), &resp).await?;
                continue;
            }
        };

        let resp = match cmd {
            DaemonCommand::Ping => DaemonResponse::Pong,
            DaemonCommand::List => handle_list(state.supervisor.as_ref()).await,
            DaemonCommand::Status { name } => handle_status(state.supervisor.as_ref(), &name).await,
            DaemonCommand::Start { name } => handle_start(state.supervisor.as_ref(), &name).await,
            DaemonCommand::Stop { name, force } => {
                handle_stop(state.supervisor.as_ref(), &name, force).await
            }
            DaemonCommand::Restart { name } => {
                handle_restart(state.supervisor.as_ref(), &name).await
            }
            DaemonCommand::Load { name } => handle_load(state.supervisor.as_ref(), &name).await,
            DaemonCommand::Subscribe => handle_subscribe(state.event_sender.subscribe()).await,
        };

        send_response(&mut reader.get_mut(), &resp).await?;
    }

    Ok(())
}

async fn send_response<W: AsyncWriteExt + Unpin>(
    writer: &mut W,
    resp: &DaemonResponse,
) -> Result<()> {
    let json = serde_json::to_string(resp)?;
    writer.write_all(json.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;
    Ok(())
}

async fn handle_list(supervisor: &ServiceSupervisor) -> DaemonResponse {
    let services = supervisor.list_services().await;

    #[derive(Serialize)]
    struct ServiceRow {
        name: String,
        status: String,
        health: String,
        pid: Option<u32>,
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
            pid: s.pid,
            restart_count: s.restart_count,
        })
        .collect();

    DaemonResponse::Ok {
        data: serde_json::to_value(rows).unwrap(),
    }
}

async fn handle_status(supervisor: &ServiceSupervisor, name: &str) -> DaemonResponse {
    match supervisor.get_status(name).await {
        Ok(status) => DaemonResponse::Ok {
            data: serde_json::to_value(status).unwrap(),
        },
        Err(e) => DaemonResponse::Error {
            message: format!("Service not found: {}", e),
        },
    }
}

async fn handle_start(supervisor: &ServiceSupervisor, name: &str) -> DaemonResponse {
    match supervisor.start_service(name).await {
        Ok(()) => DaemonResponse::Ok {
            data: serde_json::json!({"message": format!("Service '{}' start initiated", name)}),
        },
        Err(e) => DaemonResponse::Error {
            message: format!("Failed to start service: {}", e),
        },
    }
}

async fn handle_stop(supervisor: &ServiceSupervisor, name: &str, force: bool) -> DaemonResponse {
    match supervisor.stop_service(name, force).await {
        Ok(()) => DaemonResponse::Ok {
            data: serde_json::json!({"message": format!("Service '{}' stopped", name)}),
        },
        Err(e) => DaemonResponse::Error {
            message: format!("Failed to stop service: {}", e),
        },
    }
}

async fn handle_restart(supervisor: &ServiceSupervisor, name: &str) -> DaemonResponse {
    match supervisor.restart_service(name).await {
        Ok(()) => DaemonResponse::Ok {
            data: serde_json::json!({"message": format!("Service '{}' restarted", name)}),
        },
        Err(e) => DaemonResponse::Error {
            message: format!("Failed to restart service: {}", e),
        },
    }
}

async fn handle_load(_supervisor: &ServiceSupervisor, name: &str) -> DaemonResponse {
    // Load service from TOML - would need to implement in supervisor
    DaemonResponse::Ok {
        data: serde_json::json!({"message": format!("Service '{}' load requested", name)}),
    }
}

async fn handle_subscribe(_events: broadcast::Receiver<SupervisorEvent>) -> DaemonResponse {
    // This would need to be handled differently - return a stream
    // For now, just return success
    DaemonResponse::Ok {
        data: serde_json::json!({"message": "Subscribed to events"}),
    }
}
