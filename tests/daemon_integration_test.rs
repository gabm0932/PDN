//! Daemon Integration Tests
//! Tests full podnext-daemon lifecycle via Unix socket IPC

use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::process::Command;
use tokio::time::{sleep, timeout};

const DAEMON_BIN: &str =
    "/data/data/com.termux/files/home/workspace/PDN/target/debug/podnext-daemon";
const TEST_SOCKET_NAME: &str = "daemon_test.sock";

struct DaemonTestEnv {
    temp_dir: TempDir,
    socket_path: PathBuf,
    services_dir: PathBuf,
    runtime_dir: PathBuf,
    log_dir: PathBuf,
    daemon_process: Option<tokio::process::Child>,
}

impl DaemonTestEnv {
    async fn new() -> Result<Self> {
        let temp_dir = TempDir::new().context("Failed to create temp dir")?;
        let base = temp_dir.path().to_path_buf();

        let socket_path = base.join(TEST_SOCKET_NAME);
        let services_dir = base.join("services");
        let runtime_dir = base.join("runtime");
        let log_dir = base.join("logs");

        tokio::fs::create_dir_all(&services_dir).await?;
        tokio::fs::create_dir_all(&runtime_dir).await?;
        tokio::fs::create_dir_all(&log_dir).await?;

        Ok(Self {
            temp_dir,
            socket_path,
            services_dir,
            runtime_dir,
            log_dir,
            daemon_process: None,
        })
    }

    fn create_test_service(&self, name: &str, command: Vec<String>) -> Result<PathBuf> {
        let toml = format!(
            r#"
name = "{}"
description = "Test service for integration test"
version = "1.0.0"
service_type = "daemon"

[process]
command = {:?}

[restart]
condition = "on-failure"
max_restarts = 3

[health]
enabled = false
"#,
            name, command
        );

        let path = self.services_dir.join(format!("{}.toml", name));
        std::fs::write(&path, toml)?;
        Ok(path)
    }

    async fn start_daemon(&mut self) -> Result<()> {
        let mut cmd = Command::new(DAEMON_BIN);
        cmd.arg("--socket")
            .arg(&self.socket_path)
            .arg("--services-dir")
            .arg(&self.services_dir)
            .arg("--runtime-dir")
            .arg(&self.runtime_dir)
            .arg("--log-dir")
            .arg(&self.log_dir)
            .arg("--foreground")
            .arg("--interval-ms")
            .arg("100")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let child = cmd.spawn().context("Failed to spawn daemon")?;
        self.daemon_process = Some(child);

        // Wait for socket to be ready
        for _ in 0..50 {
            if self.socket_path.exists() {
                break;
            }
            sleep(Duration::from_millis(100)).await;
        }

        if !self.socket_path.exists() {
            return Err(anyhow::anyhow!("Daemon socket not created"));
        }

        // Give daemon a moment to fully initialize
        sleep(Duration::from_millis(500)).await;
        Ok(())
    }

    async fn stop_daemon(&mut self) -> Result<()> {
        if let Some(mut child) = self.daemon_process.take() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        Ok(())
    }

    fn client(&self) -> DaemonClient {
        DaemonClient::new(self.socket_path.clone())
    }
}

struct DaemonClient {
    socket_path: PathBuf,
}

impl DaemonClient {
    fn new(socket_path: PathBuf) -> Self {
        Self { socket_path }
    }

    async fn send_command(&self, cmd: Value) -> Result<Value> {
        let stream = UnixStream::connect(&self.socket_path).await?;
        let mut reader = BufReader::new(stream);
        let mut line = String::new();

        let cmd_str = serde_json::to_string(&cmd)?;
        reader.get_mut().write_all(cmd_str.as_bytes()).await?;
        reader.get_mut().write_all(b"\n").await?;
        reader.get_mut().flush().await?;

        timeout(Duration::from_secs(5), reader.read_line(&mut line)).await??;

        let resp: Value = serde_json::from_str(line.trim())?;
        Ok(resp)
    }

    async fn ping(&self) -> Result<()> {
        let resp = self.send_command(json!({"cmd": "ping"})).await?;
        assert_eq!(resp["type"], "pong");
        Ok(())
    }

    async fn list(&self) -> Result<Value> {
        let resp = self.send_command(json!({"cmd": "list"})).await?;
        assert_eq!(resp["type"], "ok");
        Ok(resp["data"].clone())
    }

    async fn status(&self, name: &str) -> Result<Value> {
        let resp = self
            .send_command(json!({"cmd": "status", "name": name}))
            .await?;
        if resp["type"] == "error" {
            return Err(anyhow::anyhow!("{}", resp["message"]));
        }
        Ok(resp["data"].clone())
    }

    async fn start(&self, name: &str) -> Result<()> {
        let resp = self
            .send_command(json!({"cmd": "start", "name": name}))
            .await?;
        if resp["type"] == "error" {
            return Err(anyhow::anyhow!("{}", resp["message"]));
        }
        Ok(())
    }

    async fn stop(&self, name: &str, force: bool) -> Result<()> {
        let resp = self
            .send_command(json!({"cmd": "stop", "name": name, "force": force}))
            .await?;
        if resp["type"] == "error" {
            return Err(anyhow::anyhow!("{}", resp["message"]));
        }
        Ok(())
    }

    async fn restart(&self, name: &str) -> Result<()> {
        let resp = self
            .send_command(json!({"cmd": "restart", "name": name}))
            .await?;
        if resp["type"] == "error" {
            return Err(anyhow::anyhow!("{}", resp["message"]));
        }
        Ok(())
    }

    async fn subscribe_events(&self) -> Result<()> {
        let resp = self.send_command(json!({"cmd": "subscribe"})).await?;
        assert_eq!(resp["type"], "ok");
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn test_daemon_lifecycle() -> Result<()> {
    let mut env = DaemonTestEnv::new().await?;

    // Create test service BEFORE starting daemon (daemon loads on startup)
    env.create_test_service("test-sleep", vec!["sleep".into(), "300".into()])?;

    env.start_daemon().await?;
    let client = env.client();

    client.ping().await?;

    let list = client.list().await?;
    assert!(list.is_array());

    // Start the service
    client.start("test-sleep").await?;

    sleep(Duration::from_millis(500)).await;

    let status = client.status("test-sleep").await?;
    // State is lowercase in daemon response
    assert_eq!(status["state"].as_str().unwrap().to_lowercase(), "running");
    assert!(status["pid"].is_u64());

    let list = client.list().await?;
    let services = list.as_array().unwrap();
    let found = services.iter().any(|s| {
        s["name"] == "test-sleep" && s["status"].as_str().unwrap().to_lowercase() == "running"
    });
    assert!(found, "Service not found in list");

    client.stop("test-sleep", false).await?;

    sleep(Duration::from_millis(500)).await;

    let status = client.status("test-sleep").await?;
    assert_eq!(status["state"].as_str().unwrap().to_lowercase(), "stopped");

    env.stop_daemon().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_daemon_subscribe_events() -> Result<()> {
    let mut env = DaemonTestEnv::new().await?;
    env.create_test_service("event-test", vec!["sleep".into(), "300".into()])?;
    env.start_daemon().await?;

    let client = env.client();

    client.subscribe_events().await?;

    client.start("event-test").await?;
    sleep(Duration::from_millis(500)).await;

    client.stop("event-test", false).await?;
    sleep(Duration::from_millis(500)).await;

    env.stop_daemon().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_daemon_restart() -> Result<()> {
    let mut env = DaemonTestEnv::new().await?;
    env.create_test_service("restart-test", vec!["sleep".into(), "300".into()])?;
    env.start_daemon().await?;

    let client = env.client();

    client.start("restart-test").await?;
    sleep(Duration::from_millis(500)).await;

    let status_before = client.status("restart-test").await?;
    let pid_before = status_before["pid"].as_u64().unwrap();

    client.restart("restart-test").await?;
    sleep(Duration::from_millis(500)).await;

    let status_after = client.status("restart-test").await?;
    let pid_after = status_after["pid"].as_u64().unwrap();

    assert_ne!(pid_before, pid_after);
    assert_eq!(
        status_after["state"].as_str().unwrap().to_lowercase(),
        "running"
    );

    env.stop_daemon().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_daemon_multiple_services() -> Result<()> {
    let mut env = DaemonTestEnv::new().await?;

    for i in 0..5 {
        let name = format!("multi-{}", i);
        env.create_test_service(&name, vec!["sleep".into(), "300".into()])?;
    }

    env.start_daemon().await?;
    let client = env.client();

    for i in 0..5 {
        let name = format!("multi-{}", i);
        client.start(&name).await?;
    }

    sleep(Duration::from_millis(500)).await;

    let list = client.list().await?;
    let services = list.as_array().unwrap();
    let running_count = services
        .iter()
        .filter(|s| s["status"].as_str().unwrap().to_lowercase() == "running")
        .count();
    assert_eq!(running_count, 5);

    for i in 0..5 {
        let name = format!("multi-{}", i);
        client.stop(&name, false).await?;
    }

    sleep(Duration::from_millis(500)).await;

    env.stop_daemon().await?;
    Ok(())
}
