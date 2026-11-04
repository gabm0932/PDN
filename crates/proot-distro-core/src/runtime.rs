//! Unified Container Runtime Backend Abstraction
//!
//! Provides a polymorphic execution interface for container operations via
//! PRoot user-space emulation or User-Mode Linux (UML) kernel backends.

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::sync::Mutex;

use crate::container::Container;
use proot_core::{ProotConfig, ProotEngine};

/// Supported container runtime backend variants
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeKind {
    /// PRoot ptrace chroot emulation (default, unprivileged user-space)
    Proot,
    /// User-Mode Linux (genuine Linux kernel supporting namespaces, cgroups v2, Docker)
    Uml,
}

impl Default for RuntimeKind {
    fn default() -> Self {
        Self::Proot
    }
}

/// Runtime health status and capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeStatus {
    pub kind: RuntimeKind,
    pub available: bool,
    pub running: bool,
    pub supports_kernel_features: bool,
    pub details: String,
}

/// Polymorphic execution interface for container runtime backends
#[async_trait]
pub trait RuntimeBackend: Send + Sync {
    /// Returns runtime kind identity
    fn kind(&self) -> RuntimeKind;

    /// Indicates whether this backend provides genuine kernel namespaces, cgroups, and overlayfs
    fn supports_kernel_features(&self) -> bool;

    /// Executes a command in the container environment and returns process exit code
    async fn execute(&self, container: &Container, command: Vec<String>) -> Result<i32>;

    /// Queries runtime backend health and readiness status
    async fn status(&self) -> Result<RuntimeStatus>;
}

/// PRoot user-space ptrace chroot backend
pub struct ProotBackend {
    engine: Arc<Mutex<Option<ProotEngine>>>,
}

impl ProotBackend {
    /// Create new PRoot backend with uninitialized engine
    pub fn new() -> Self {
        Self {
            engine: Arc::new(Mutex::new(None)),
        }
    }

    /// Create PRoot backend reusing an existing engine holder
    pub fn with_engine(engine: Arc<Mutex<Option<ProotEngine>>>) -> Self {
        Self { engine }
    }

    async fn get_engine(&self) -> Result<Arc<Mutex<ProotEngine>>> {
        let mut engine_guard = self.engine.lock().await;
        if engine_guard.is_none() {
            let config = ProotConfig::default();
            *engine_guard = Some(ProotEngine::new(config)?);
        }
        let engine = engine_guard.as_ref().unwrap();
        Ok(Arc::new(Mutex::new(engine.clone())))
    }
}

impl Default for ProotBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl RuntimeBackend for ProotBackend {
    fn kind(&self) -> RuntimeKind {
        RuntimeKind::Proot
    }

    fn supports_kernel_features(&self) -> bool {
        false
    }

    async fn execute(&self, container: &Container, command: Vec<String>) -> Result<i32> {
        let engine = self.get_engine().await?;
        let mut engine_guard = engine.lock().await;

        // Configure proot from container config
        engine_guard.config.rootfs = Some(container.rootfs_path.clone());
        engine_guard.config.isolated = container.config.isolated;
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

        // Synthetic /proc entries and guest runtime directories
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
        engine_guard.config.seccomp = container.config.security.seccomp_profile.is_some();
        engine_guard.config.kernel_release = container.config.kernel_release.clone();
        engine_guard.config.hostname = container.config.hostname.clone();

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

        // Publish a live session record for this foreground run/login so
        // `podnext ps` reports the workload while the proot child is alive
        // (issue #14). The returned File holds the LOCK_EX on the session file
        // and is inherited (FD_CLOEXEC cleared) by the descendant; dropping it
        // after the child exits releases the lock, and the next
        // `active_sessions()` prunes the stale record.
        // ponytail: pid is this process, not the proot grandchild — proot-core
        // does not expose the child pid. Same lifetime, so the flock probe is
        // still exact; revisit if the engine returns its child pid.
        let _session = crate::session::register_session(
            &container.name,
            std::process::id(),
            "0",
            &engine_guard.config.command,
        )
        .ok();

        let exit_code = engine_guard.execute().await? as i32;
        Ok(exit_code)
    }

    async fn status(&self) -> Result<RuntimeStatus> {
        Ok(RuntimeStatus {
            kind: RuntimeKind::Proot,
            available: true,
            running: true,
            supports_kernel_features: false,
            details: "PRoot user-space ptrace chroot emulation".to_string(),
        })
    }
}

/// Resolves standard path to uml4termux daemon control socket
fn default_uml_socket_path() -> PathBuf {
    if let Ok(runtime) = std::env::var("UML4TERMUX_RUNTIME") {
        let p = PathBuf::from(runtime).join("daemon.sock");
        if p.exists() {
            return p;
        }
    }
    let fallback_fixed =
        PathBuf::from("/data/data/com.termux/files/usr/tmp/uml4termux/daemon.sock");
    if fallback_fixed.exists() {
        return fallback_fixed;
    }
    if let Ok(tmpdir) = std::env::var("TMPDIR") {
        let p = PathBuf::from(tmpdir).join("uml4termux").join("daemon.sock");
        if p.exists() {
            return p;
        }
    }
    if let Ok(runtime) = std::env::var("UML4TERMUX_RUNTIME") {
        PathBuf::from(runtime).join("daemon.sock")
    } else if let Ok(tmpdir) = std::env::var("TMPDIR") {
        PathBuf::from(tmpdir).join("uml4termux").join("daemon.sock")
    } else {
        fallback_fixed
    }
}

/// Parses command exit code from UML console log output
pub fn parse_uml_exec_exit_code(content: &str, seq: u64) -> Option<i32> {
    let begin_marker = format!("__EXEC_{}_BEGIN__", seq);
    let done_marker = format!("__EXEC_{}_DONE_", seq);

    let begin_pos = content.rfind(&begin_marker)?;
    let after_begin = &content[begin_pos..];
    let done_pos = after_begin.find(&done_marker)?;
    let rest = &after_begin[done_pos + done_marker.len()..];

    if let Some(nl_pos) = rest.find(['\n', '\r']) {
        let code_candidate = rest[..nl_pos].trim();
        code_candidate.parse::<i32>().ok()
    } else {
        let digits: String = rest
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '-')
            .collect();
        if !digits.is_empty() {
            digits.parse::<i32>().ok()
        } else {
            None
        }
    }
}

/// User-Mode Linux (UML) daemon client runtime backend
pub struct UmlBackend {
    socket_path: PathBuf,
}

impl UmlBackend {
    /// Create new UML backend using environment or standard fallback paths
    pub fn new() -> Self {
        Self {
            socket_path: default_uml_socket_path(),
        }
    }

    /// Create UML backend targeting explicit socket path
    pub fn with_socket(socket_path: PathBuf) -> Self {
        Self { socket_path }
    }

    /// Current socket path
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Send one raw command line and return daemon response
    pub async fn send_command(&self, line: &str) -> Result<String> {
        let mut stream = tokio::time::timeout(
            Duration::from_secs(10),
            UnixStream::connect(&self.socket_path),
        )
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "Timeout connecting to uml4termuxd socket at {}",
                self.socket_path.display()
            )
        })?
        .with_context(|| {
            format!(
                "uml4termuxd socket not reachable at {}",
                self.socket_path.display()
            )
        })?;

        let mut payload = line.to_string();
        payload.push('\n');
        stream.write_all(payload.as_bytes()).await?;
        stream.flush().await?;
        let _ = stream.shutdown().await;

        let mut out = String::new();
        tokio::time::timeout(Duration::from_secs(15), stream.read_to_string(&mut out))
            .await
            .map_err(|_| anyhow::anyhow!("Timeout reading response from uml4termuxd"))?
            .with_context(|| "Failed reading response from uml4termuxd")?;

        Ok(out.trim_end_matches(['\n', '\r']).to_string())
    }

    /// Send command with automatic rate-limit backoff retry
    pub async fn request_with_retry(&self, line: &str) -> Result<serde_json::Value> {
        let max_retries = 5;
        let mut attempts = 0;
        loop {
            attempts += 1;
            let resp_str = self.send_command(line).await?;
            let val: serde_json::Value = serde_json::from_str(&resp_str)
                .with_context(|| format!("Invalid JSON from uml4termuxd: {}", resp_str))?;

            if let Some(err) = val.get("error").and_then(|e| e.as_str()) {
                if (err == "rate-limited" || resp_str.contains("rate-limited"))
                    && attempts <= max_retries
                {
                    let wait_ms = val
                        .get("min_interval_ms")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(100);
                    tokio::time::sleep(Duration::from_millis(wait_ms + 10)).await;
                    continue;
                }
                if err == "command too long" {
                    bail!("Command exceeds uml4termux limit of 2000 bytes");
                }
                bail!("uml4termuxd error: {}", err);
            }
            return Ok(val);
        }
    }

    /// Perform runtime protocol handshake query
    pub async fn protocol(&self) -> Result<serde_json::Value> {
        self.request_with_retry("protocol").await
    }
}

impl Default for UmlBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl RuntimeBackend for UmlBackend {
    fn kind(&self) -> RuntimeKind {
        RuntimeKind::Uml
    }

    fn supports_kernel_features(&self) -> bool {
        true
    }

    async fn execute(&self, _container: &Container, command: Vec<String>) -> Result<i32> {
        let joined_cmd = if command.is_empty() {
            "/bin/sh".to_string()
        } else {
            command.join(" ")
        };

        if joined_cmd.len() > 2000 {
            bail!(
                "Command too long for uml4termux console channel (limit 2000 bytes, got {})",
                joined_cmd.len()
            );
        }

        let exec_req = format!("exec {}", joined_cmd);
        let resp = self.request_with_retry(&exec_req).await?;

        let seq = resp
            .get("seq")
            .and_then(|s| s.as_u64())
            .context("Missing seq in uml4termuxd exec response")?;

        let console_path = resp
            .get("console")
            .and_then(|c| c.as_str())
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                if let Ok(runtime) = std::env::var("UML4TERMUX_RUNTIME") {
                    PathBuf::from(runtime).join("uml-console.log")
                } else if let Some(parent) = self.socket_path.parent() {
                    parent.join("uml-console.log")
                } else if let Ok(tmpdir) = std::env::var("TMPDIR") {
                    PathBuf::from(tmpdir)
                        .join("uml4termux")
                        .join("uml-console.log")
                } else {
                    PathBuf::from("/data/data/com.termux/files/usr/tmp/uml4termux/uml-console.log")
                }
            });

        let timeout = Duration::from_secs(300);
        let start = std::time::Instant::now();
        let mut exit_code = None;

        while start.elapsed() < timeout {
            if let Ok(content) = tokio::fs::read_to_string(&console_path).await {
                if let Some(rc) = parse_uml_exec_exit_code(&content, seq) {
                    exit_code = Some(rc);
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(150)).await;
        }

        exit_code.context("Timed out waiting for command execution completion in UML guest")
    }

    async fn status(&self) -> Result<RuntimeStatus> {
        if !self.socket_path.exists() {
            return Ok(RuntimeStatus {
                kind: RuntimeKind::Uml,
                available: false,
                running: false,
                supports_kernel_features: true,
                details: format!(
                    "uml4termuxd socket does not exist at {} (run 'uml4termux up' first)",
                    self.socket_path.display()
                ),
            });
        }

        match self.send_command("status").await {
            Ok(reply) => {
                let v: serde_json::Value = serde_json::from_str(&reply)
                    .unwrap_or_else(|_| serde_json::json!({ "raw": reply }));
                let state = v.get("state").and_then(|s| s.as_str()).unwrap_or("unknown");
                let is_running = state == "running";
                Ok(RuntimeStatus {
                    kind: RuntimeKind::Uml,
                    available: true,
                    running: is_running,
                    supports_kernel_features: true,
                    details: format!("uml4termuxd status: state={}, info={}", state, reply),
                })
            }
            Err(e) => Ok(RuntimeStatus {
                kind: RuntimeKind::Uml,
                available: false,
                running: false,
                supports_kernel_features: true,
                details: format!("Cannot connect to uml4termuxd: {}", e),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_kind_serde() {
        let proot_json = serde_json::to_string(&RuntimeKind::Proot).unwrap();
        assert_eq!(proot_json, "\"proot\"");

        let uml_json = serde_json::to_string(&RuntimeKind::Uml).unwrap();
        assert_eq!(uml_json, "\"uml\"");

        let de_proot: RuntimeKind = serde_json::from_str("\"proot\"").unwrap();
        assert_eq!(de_proot, RuntimeKind::Proot);

        let de_uml: RuntimeKind = serde_json::from_str("\"uml\"").unwrap();
        assert_eq!(de_uml, RuntimeKind::Uml);

        assert_eq!(RuntimeKind::default(), RuntimeKind::Proot);
    }

    #[test]
    fn test_backend_capabilities() {
        let proot = ProotBackend::new();
        assert_eq!(proot.kind(), RuntimeKind::Proot);
        assert!(!proot.supports_kernel_features());

        let uml = UmlBackend::new();
        assert_eq!(uml.kind(), RuntimeKind::Uml);
        assert!(uml.supports_kernel_features());
    }

    #[tokio::test]
    async fn test_uml_status_offline() {
        let backend = UmlBackend::with_socket(PathBuf::from("/nonexistent/uml4termux/daemon.sock"));
        let st = backend.status().await.unwrap();
        assert_eq!(st.kind, RuntimeKind::Uml);
        assert!(!st.available);
        assert!(!st.running);
        assert!(st.supports_kernel_features);
    }

    #[test]
    fn test_parse_uml_exec_exit_code() {
        let log = r#"
exec:echo __EXEC_1_BEGIN__; echo hello; echo __EXEC_1_DONE_$?
__AGENT_GOT__: echo __EXEC_1_BEGIN__; echo hello; echo __EXEC_1_DONE_$?
__EXEC_1_BEGIN__
hello
__EXEC_1_DONE_0
"#;
        assert_eq!(parse_uml_exec_exit_code(log, 1), Some(0));

        let fail_log = r#"
__EXEC_2_BEGIN__
error encountered
__EXEC_2_DONE_127
"#;
        assert_eq!(parse_uml_exec_exit_code(fail_log, 2), Some(127));

        // Different sequence not completed
        assert_eq!(parse_uml_exec_exit_code(fail_log, 3), None);

        // Negative exit code
        let neg_log = "__EXEC_4_BEGIN__\ninterrupted\n__EXEC_4_DONE_-1\n";
        assert_eq!(parse_uml_exec_exit_code(neg_log, 4), Some(-1));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_container_manager_get_backend() {
        let manager = crate::container::ContainerManager::new().unwrap();
        let mut config = crate::container::ContainerConfig::default();
        assert_eq!(config.runtime, RuntimeKind::Proot);

        let c_proot =
            crate::container::Container::new("test-proot".to_string(), config.clone()).unwrap();
        let backend_proot = manager.get_backend(&c_proot);
        assert_eq!(backend_proot.kind(), RuntimeKind::Proot);
        assert!(!backend_proot.supports_kernel_features());

        config.runtime = RuntimeKind::Uml;
        let c_uml = crate::container::Container::new("test-uml".to_string(), config).unwrap();
        let backend_uml = manager.get_backend(&c_uml);
        assert_eq!(backend_uml.kind(), RuntimeKind::Uml);
        assert!(backend_uml.supports_kernel_features());
    }
}
