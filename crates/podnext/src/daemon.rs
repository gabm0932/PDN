//! podnext-daemon - Persistent service supervisor daemon
//!
//! Runs as a background daemon managing container services via Unix socket IPC.

use anyhow::{Context, Result};
use clap::Parser;
use pn_next::{ServiceSupervisor, SupervisorBuilder, SupervisorConfig, SupervisorEvent};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::signal::unix::{signal, Signal, SignalKind};
use tokio::sync::broadcast;
use tokio::task::JoinSet;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

/// Bounded grace period for in-flight connections after a shutdown signal.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(10);

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

/// What to do when a signal arrives.
#[derive(Debug, PartialEq, Eq)]
enum SignalAction {
    /// Stop the daemon (SIGTERM from a supervisor, SIGINT from Ctrl-C).
    Shutdown,
    /// Keep running (SIGHUP: reload is not implemented).
    Ignore,
}

/// Map a received signal to an action.
///
/// SIGTERM and SIGINT are shutdown. SIGHUP conventionally means "reload your
/// config", but there is no reload path here (a supervisor restart would need
/// to rebuild the dependency graph); the safe choice is to ignore it rather
/// than die, so a stray terminal-hangup or `kill -HUP` cannot take the daemon
/// down silently.
fn signal_action(kind: SignalKind) -> SignalAction {
    if kind == SignalKind::terminate() || kind == SignalKind::interrupt() {
        SignalAction::Shutdown
    } else {
        SignalAction::Ignore
    }
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

    // Install signal handlers before any async startup work: a supervisor that
    // stops the daemon immediately after spawn sends SIGTERM while we are still
    // initializing, and an unregistered SIGTERM would kill the process.
    let mut sigterm = signal(SignalKind::terminate()).context("install SIGTERM handler")?;
    let mut sigint = signal(SignalKind::interrupt()).context("install SIGINT handler")?;
    let mut sighup = signal(SignalKind::hangup()).context("install SIGHUP handler")?;

    let shutdown = async move {
        loop {
            let kind = tokio::select! {
                _ = sigterm.recv() => SignalKind::terminate(),
                _ = sigint.recv() => SignalKind::interrupt(),
                _ = sighup.recv() => SignalKind::hangup(),
            };
            match signal_action(kind) {
                SignalAction::Shutdown => {
                    info!("Shutdown signal received ({:?})", kind);
                    return;
                }
                SignalAction::Ignore => {
                    info!("SIGHUP received; reload not implemented, ignoring");
                }
            }
        }
    };

    run(args, shutdown).await
}

/// Run the daemon until `shutdown` resolves, then tear down and return normally.
///
/// Returning `Ok(())` maps to process exit status 0 (a clean stop a supervisor
/// can trust); any setup failure returns `Err` and a non-zero status.
async fn run(args: Args, shutdown: impl std::future::Future<Output = ()>) -> Result<()> {
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

    tokio::pin!(shutdown);

    // Accept connections until a shutdown signal arrives.
    let mut connections: JoinSet<()> = JoinSet::new();
    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => {
                    let state = state.clone();
                    connections.spawn(async move {
                        if let Err(e) = handle_connection(stream, state).await {
                            warn!("Connection error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    error!("Accept error: {}", e);
                }
            },
        }
    }

    // Stop accepting: drop the listener so the socket path stops taking connections.
    drop(listener);

    // Let in-flight connections finish, bounded by a grace period.
    let drained = tokio::time::timeout(SHUTDOWN_GRACE, async {
        while connections.join_next().await.is_some() {}
    })
    .await;

    if drained.is_err() {
        warn!(
            "Grace period elapsed; aborting {} in-flight connection(s)",
            connections.len()
        );
        connections.abort_all();
        while connections.join_next().await.is_some() {}
    }

    // Clean up the socket file now that we own the shutdown.
    if let Err(e) = std::fs::remove_file(&args.socket) {
        if e.kind() != std::io::ErrorKind::NotFound {
            warn!("Failed to remove socket {:?}: {}", args.socket, e);
        }
    }

    info!("podnext-daemon shut down cleanly");
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sigterm_and_sigint_shutdown_sighup_is_ignored() {
        assert_eq!(
            signal_action(SignalKind::terminate()),
            SignalAction::Shutdown
        );
        assert_eq!(
            signal_action(SignalKind::interrupt()),
            SignalAction::Shutdown
        );
        assert_eq!(signal_action(SignalKind::hangup()), SignalAction::Ignore);
    }

    #[tokio::test]
    async fn graceful_shutdown_removes_socket_and_returns_ok() {
        let dir = tempfile::tempdir().unwrap();
        let args = Args {
            socket: dir.path().join("daemon.sock"),
            services_dir: dir.path().join("services"),
            runtime_dir: dir.path().join("run"),
            log_dir: dir.path().join("log"),
            interval_ms: 1000,
            enable_cgroups: false,
            foreground: true,
        };
        let socket = args.socket.clone();

        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let daemon = tokio::spawn(run(args, async move {
            let _ = rx.await;
        }));

        // Poll for readiness on state, not on a fixed duration.
        let mut bound = false;
        for _ in 0..250 {
            if socket.exists() {
                bound = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(bound, "daemon never bound the socket");

        tx.send(()).unwrap();
        let result = daemon.await.expect("daemon task panicked");
        assert!(
            result.is_ok(),
            "clean shutdown should return Ok, got {result:?}"
        );
        assert!(!socket.exists(), "socket file was not removed on shutdown");
    }
}
