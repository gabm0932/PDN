# Docker Runtime Integration Blueprint in PDN: Architecture & RuntimeBackend Implementation

**Status:** Implementation Blueprint & Technical Architecture
**Target Files:** `crates/proot-distro-core/src/runtime.rs`, `container.rs`, `lib.rs`, `crates/podnext/src/main.rs`
**Host Environment:** Android 16 (API 36), aarch64, Linux 6.1.157, Termux unprivileged (`uid=10396`, `CapEff=0`)

---

## 1. Structural Comparison: Docker vs PRoot

### 1.1 Why Docker Cannot Run Under PRoot
PRoot operates purely in user-space via `ptrace(2)` system call tracing (`PTRACE_SYSCALL`). PRoot is not a kernel; it merely intercepts select file-path and process-identity syscalls to simulate `chroot` and `fake root` (`uid=0`, `gid=0`).

Docker (`dockerd`) and OCI container runtimes (`runc`) cannot function merely through user-space path translation. Docker strictly requires genuine kernel-level virtualization and process isolation.

### 1.2 System Call Audit: Docker Requirements vs Android PRoot Reality

| Syscall / Feature | Docker / runc Requirement | Measured Result on Android Termux (Host) | PRoot Behavior | Impact on Docker |
|---|---|---|---|---|
| `mount(2)` (overlayfs) | Mounts container image layers (`lowerdir`, `upperdir`, `workdir`) | **SIGSYS (Signal 31)** from Android seccomp, or `EPERM` (`CapEff=0`) | Intercepts syscall and returns fake `rc=0` without creating genuine VFS mounts | `dockerd` assumes mount succeeded, but the merged directory is empty; runc crashes during rootfs initialization |
| `mount(2)` (`cgroup2`, `tmpfs`) | Mounts `/sys/fs/cgroup`, `/run`, `/dev/shm` | **SIGSYS / EPERM** | Returns fake `rc=0` or no-op | Init systems and control groups fail to attach to filesystems |
| `unshare(CLONE_NEWUSER)` | Allocates user namespace (rootless mode) | **EINVAL** (Android kernel disables unprivileged user namespaces) | Incapable of virtualizing user namespaces | Rootless Docker fails initialization |
| `unshare(CLONE_NEWNS \| CLONE_NEWPID \| CLONE_NEWNET)` | Isolates filesystem, process tree, and network stack | **EPERM** (requires host `CAP_SYS_ADMIN`) | Strips namespace flags or acts as no-op; processes remain in host namespace | Containers have zero isolation; PID and port collisions occur |
| `pivot_root(2)` | Swaps rootfs and detaches host filesystem | **EINVAL / EPERM** (requires `CAP_SYS_ADMIN` and genuine mountpoint) | Diverted to standard chroot or fails | `runc` verifies mountpoint topology and aborts container sandbox setup |
| Write to cgroups v2 (`/sys/fs/cgroup`) | Writes to `cgroup.procs`, `cpu.max`, `memory.max`, `pids.max` | **Permission denied (`EACCES`)** (owned by Android system/root) | No cgroup emulation in ptrace | Containers fail resource governance; dockerd refuses to launch without a valid cgroup driver |

### 1.3 The PRoot Deception Trap
When applications call `mount(2)` inside PRoot, PRoot intercepts the call and returns `0` (success). Consequently:
1. `dockerd` assumes the `overlay2` storage driver is active and functional.
2. When the first container is spawned, layer files are never actually unified by the kernel VFS. Binary executables inside the container are missing or unexecutable.
3. Attempting to run Docker inside pure PRoot is an architecturally flawed premise from the outset.

### 1.4 The User-Mode Linux (UML) Solution
`uml4termux` runs a genuine Linux kernel (`linux-bionic-smp`, arm64) as a single unprivileged Android user process:
- The guest kernel manages its own virtual page tables, process scheduler, VFS, networking stack, and cgroup controllers.
- Inside the guest, processes execute with genuine root capabilities (`CapEff = 0x1fffffffff`).
- Syscalls like `mount(overlay)`, `unshare(CLONE_NEWNS)`, `pivot_root`, and writes to `/sys/fs/cgroup` are executed directly by the UML guest kernel, not the host Android kernel.
- The Android host only observes a standard compute thread and a loopback networking socket (`gvproxy`).

---

## 2. Daemon Interface Audit: `uml4termuxd`

### 2.1 Architecture & Communication Topology
The supervisor daemon `uml4termuxd` (or its twin `uml4termux-rs daemon`) governs the lifecycle of the UML VM and exposes a local UNIX domain socket.

```
┌────────────────────────────────────────────────────────┐
│ Host: Termux Environment                               │
│                                                        │
│  podnext CLI / PDN Core                                │
│       │                                                │
│       ▼ (UNIX Domain Socket: 0600, SO_PEERCRED)        │
│  uml4termuxd (Supervisor Daemon)                       │
│       │                 │                              │
│       │ (PTY Master)    │ (BESS Socket)                │
│       ▼                 ▼                              │
│  linux-bionic-smp    gvproxy (User-mode Networking)    │
│  (UML Guest Kernel)     │ (Port Forwarder 127.0.0.1)   │
│       │                 │                              │
│       ├─ dockerd        │                              │
│       └─ console agent ─┘                              │
└────────────────────────────────────────────────────────┘
```

- **Socket Path:** `${UML4TERMUX_RUNTIME}/daemon.sock` (Default: `/data/data/com.termux/files/usr/tmp/uml4termux/daemon.sock`).
- **File Permissions:** Mode `0600`, daemon umask `0077`.
- **Authentication:** Prevents cross-UID connections via `SO_PEERCRED` verification (`cr.uid == geteuid()`). Every request is logged in `${UML4TERMUX_RUNTIME}/audit.log`.

### 2.2 Wire Protocol (Runtime Contract v1)
Clients send **a single line of text** terminated by `\n`. The daemon responds with **a single JSON line** terminated by `\n`, then shuts down the write stream.

Protocol Commands:
1. `protocol` -> `{"protocol":1,"runtime_contract":1,"commands":["status","stop","ports","expose","exec","protocol"]}`
2. `status` -> `{"state":"running"|"completed"|"idle","child_pid":12345,"restarts":0,"kernel":"...","mem":"1024M","ncpus":"4","uptime_s":120}`
3. `stop` -> `{"state":"stopping"}` (sends SIGTERM/sysrq to guest)
4. `ports` -> `{"exposures":["127.0.0.1:8080 -> 192.168.127.2:80"]}`
5. `expose <host_port> <guest_port>` -> `{"exposed":"127.0.0.1:<host_port> -> 192.168.127.2:<guest_port>"}`
6. `exec <command>` -> Executes command in guest shell via PTY console channel.

### 2.3 Command Wrapping (`exec`) & Output Extraction
The daemon `uml4termuxd` does not stream stdout/stderr directly over the control socket. The control socket confirms execution:
```c
seq = ++g_exec_seq;
snprintf(line, sizeof(line),
         "exec:echo __EXEC_%d_BEGIN__; %s; echo __EXEC_%d_DONE_$?\n",
         seq, req + 5, seq);
write(g_ptym, line, strlen(line));
snprintf(resp, sizeof(resp),
         "{\"started\":true,\"seq\":%d,\"console\":\"%s\"}\n",
         seq, g_consolelog);
respond(fd, resp);
```

#### Guest Execution Flow (`docker-init.sh`):
The console agent reads from `/dev/console`:
```sh
( while IFS= read -r aline; do
    case "$aline" in
      exec:*)
        printf '__AGENT_GOT__: %s\n' "${aline#exec:}"
        ( eval "${aline#exec:}" ) &
        ;;
    esac
  done ) < /dev/console &
```
1. Agent emits: `__AGENT_GOT__: echo __EXEC_<seq>_BEGIN__; <cmd>; echo __EXEC_<seq>_DONE_$?`
2. Agent evaluates the line inside a subshell `( eval ... ) &`.
3. Guest prints start marker: `__EXEC_<seq>_BEGIN__`.
4. Output (stdout/stderr) streams to `/dev/console`.
5. Guest prints completion marker with exit code: `__EXEC_<seq>_DONE_$?` (e.g. `__EXEC_1_DONE_0`).
6. Daemon writes PTY master output to `${UML4TERMUX_RUNTIME}/uml-console.log`.

#### Client-side Parsing Strategy (PDN Runtime):
- Open `${UML4TERMUX_RUNTIME}/uml-console.log`.
- Locate the line immediately following `__EXEC_<seq>_BEGIN__`.
- Accumulate output until reaching `__EXEC_<seq>_DONE_<rc>`.
- Extract integer `<rc>` as the process exit code.

### 2.4 Safety Invariants & Error Handling
1. **Command Line Limit (`WM_EXEC_MAX` = 2000 bytes):** Commands exceeding 2000 bytes are rejected with `{"error":"command too long","limit":2000}`. PDN client validates length prior to sending.
2. **Rate Limiting (`exec_rate_limited`):** Controlled by `UML4TERMUXD_EXEC_MIN_INTERVAL_MS` (default 100ms). Returns `{"error":"rate-limited","min_interval_ms":100}`. PDN client performs backoff retry.
3. **Guest Readiness (`guest not running`):** Returned if UML child process has not started or PTY is closed.
4. **Exclusive Backing Image Lock (`F_SETLK`):** UML takes an exclusive advisory lock on the image. Multiple instances must use copy-on-write overlays (`UML4TERMUXD_COW`).

---

## 3. Design Step 1: `RuntimeBackend` Trait for PDN

### 3.1 Module Definition (`crates/proot-distro-core/src/runtime.rs`)

```rust
//! Runtime Backend Abstraction for PDN (PRoot Distro Next)
//!
//! Provides a unified interface for executing containers via either
//! standard PRoot ptrace emulation or User-Mode Linux (UML) kernel backends.

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use crate::container::Container;

/// Supported runtime backend variants
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeKind {
    /// Lightweight PRoot ptrace chroot (default, no genuine kernel namespaces)
    Proot,
    /// User-Mode Linux (genuine Linux kernel supporting overlayfs, cgroups v2, Docker)
    Uml,
}

impl Default for RuntimeKind {
    fn default() -> Self {
        RuntimeKind::Proot
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

/// Primary container execution interface
#[async_trait]
pub trait RuntimeBackend: Send + Sync {
    /// Returns runtime kind identity
    fn kind(&self) -> RuntimeKind;

    /// Indicates whether this backend provides a genuine Linux kernel
    /// supporting namespaces, cgroups v2, and overlayfs (required for Docker).
    fn supports_kernel_features(&self) -> bool;

    /// Executes a command inside the container environment
    async fn execute(&self, container: &Container, command: Vec<String>) -> Result<i32>;

    /// Queries backend readiness status
    async fn status(&self) -> Result<RuntimeStatus>;
}
```

### 3.2 `ProotBackend` Implementation
Encapsulates existing PRoot execution logic from `ContainerManager::execute`:

```rust
use crate::sysdata;
use crate::guestdirs;
use proot_core::{ProotConfig, ProotEngine};
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct ProotBackend {
    engine: Arc<Mutex<Option<ProotEngine>>>,
}

impl ProotBackend {
    pub fn new() -> Self {
        Self {
            engine: Arc::new(Mutex::new(None)),
        }
    }

    async fn get_engine(&self) -> Result<Arc<Mutex<ProotEngine>>> {
        let mut guard = self.engine.lock().await;
        if guard.is_none() {
            *guard = Some(ProotEngine::new(ProotConfig::default())?);
        }
        Ok(Arc::new(Mutex::new(guard.as_ref().unwrap().clone())))
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

        engine_guard.config.rootfs = Some(container.rootfs_path.clone());
        engine_guard.config.bind = container
            .config
            .bind_mounts
            .iter()
            .map(|b| format!("{}:{}:{}", b.source, b.target, if b.readonly { "ro" } else { "rw" }))
            .collect();

        if let Some(container_dir) = container.rootfs_path.parent() {
            if sysdata::ensure(container_dir).is_ok() {
                engine_guard.config.bind.extend(sysdata::bindings(container_dir));
            }
            if guestdirs::ensure(container_dir).is_ok() {
                engine_guard.config.bind.extend(guestdirs::bindings(container_dir));
            }
        }

        engine_guard.config.cwd = Some(
            container.config.working_dir.clone().unwrap_or_else(|| "/".to_string()).into(),
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

        engine_guard.config.command = if command.is_empty() {
            vec!["/bin/sh".to_string(), "-c".to_string(), "exec /bin/sh".to_string()]
        } else {
            command
        };

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
```

### 3.3 `UmlBackend` Implementation
Client communicating with `uml4termuxd` over UNIX domain sockets:

```rust
pub struct UmlBackend {
    socket_path: PathBuf,
}

impl UmlBackend {
    pub fn new() -> Self {
        let runtime_dir = std::env::var("UML4TERMUX_RUNTIME")
            .unwrap_or_else(|_| format!("{}/uml4termux", std::env::var("TMPDIR").unwrap_or_else(|_| "/tmp".to_string())));
        Self {
            socket_path: PathBuf::from(runtime_dir).join("daemon.sock"),
        }
    }

    pub fn with_socket(path: PathBuf) -> Self {
        Self { socket_path: path }
    }

    async fn send_command(&self, line: &str) -> Result<String> {
        let mut stream = UnixStream::connect(&self.socket_path)
            .await
            .with_context(|| format!("UML daemon socket not reachable at {}", self.socket_path.display()))?;

        let mut payload = line.to_string();
        payload.push('\n');
        stream.write_all(payload.as_bytes()).await?;
        stream.flush().await?;
        stream.shutdown(std::net::Shutdown::Write)?;

        let mut reader = BufReader::new(stream);
        let mut resp = String::new();
        reader.read_line(&mut resp).await?;
        Ok(resp.trim_end().to_string())
    }

    async fn request_with_retry(&self, line: &str) -> Result<serde_json::Value> {
        let max_retries = 5;
        let mut attempts = 0;
        loop {
            attempts += 1;
            let resp_str = self.send_command(line).await?;
            let val: serde_json::Value = serde_json::from_str(&resp_str)
                .with_context(|| format!("Invalid JSON from uml4termuxd: {}", resp_str))?;

            if let Some(err) = val.get("error").and_then(|e| e.as_str()) {
                if err == "rate-limited" && attempts <= max_retries {
                    let wait_ms = val.get("min_interval_ms").and_then(|v| v.as_u64()).unwrap_or(100);
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
        let joined_cmd = command.join(" ");
        if joined_cmd.len() > 2000 {
            bail!("Command too long for uml4termux console channel (limit 2000 bytes, got {})", joined_cmd.len());
        }

        let exec_req = format!("exec {}", joined_cmd);
        let resp = self.request_with_retry(&exec_req).await?;

        let seq = resp.get("seq").and_then(|s| s.as_u64())
            .context("Missing seq in uml4termuxd exec response")?;
        let console_path = resp.get("console").and_then(|c| c.as_str())
            .map(PathBuf::from)
            .unwrap_or_else(|| self.socket_path.parent().unwrap().join("uml-console.log"));

        let done_marker = format!("__EXEC_{}_DONE_", seq);
        let begin_marker = format!("__EXEC_{}_BEGIN__", seq);

        let timeout = Duration::from_secs(300);
        let start = std::time::Instant::now();
        let mut exit_code = None;

        while start.elapsed() < timeout {
            if let Ok(content) = std::fs::read_to_string(&console_path) {
                if let Some(begin_pos) = content.rfind(&begin_marker) {
                    let after_begin = &content[begin_pos..];
                    if let Some(done_pos) = after_begin.find(&done_marker) {
                        let rest = &after_begin[done_pos + done_marker.len()..];
                        let code_str: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '-').collect();
                        if let Ok(rc) = code_str.parse::<i32>() {
                            exit_code = Some(rc);
                            break;
                        }
                    }
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
                details: "uml4termuxd socket does not exist (run 'uml4termux up' first)".into(),
            });
        }

        match self.send_command("status").await {
            Ok(reply) => {
                let v: serde_json::Value = serde_json::from_str(&reply)?;
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
```

### 3.4 Delegation in `ContainerManager::execute`
In `crates/proot-distro-core/src/container.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerConfig {
    pub name: String,
    pub distro: String,
    pub version: String,
    #[serde(default)]
    pub runtime: crate::runtime::RuntimeKind, // "proot" | "uml"
    // ...
}

impl ContainerManager {
    pub fn get_backend(&self, container: &Container) -> Box<dyn crate::runtime::RuntimeBackend> {
        match container.config.runtime {
            crate::runtime::RuntimeKind::Proot => Box::new(crate::runtime::ProotBackend::new()),
            crate::runtime::RuntimeKind::Uml => Box::new(crate::runtime::UmlBackend::new()),
        }
    }

    pub async fn execute(&self, name: &str, command: Vec<String>) -> Result<i32> {
        let container = self.get(name).await?;
        let backend = self.get_backend(&container);
        backend.execute(&container, command).await
    }
}
```

### 3.5 Implementation of `podnext docker` Subcommand
In `crates/podnext/src/main.rs`:

```rust
#[derive(Subcommand, Debug)]
enum Commands {
    // ...
    /// Run Docker commands transparently via kernel-capable runtime (UML)
    Docker {
        #[arg(short, long)]
        container: Option<String>,

        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

async fn cmd_docker(container_name: Option<String>, args: Vec<String>) -> Result<()> {
    let container_mgr = proot_distro_core::ContainerManager::new()?;

    let backend: Box<dyn proot_distro_core::runtime::RuntimeBackend> = if let Some(ref name) = container_name {
        let c = container_mgr.get(name).await?;
        container_mgr.get_backend(&c)
    } else {
        Box::new(proot_distro_core::runtime::UmlBackend::new())
    };

    if !backend.supports_kernel_features() {
        bail!(
            "The configured runtime backend '{}' does not support kernel features required by Docker.\n\
             Reason: Docker requires namespaces (mount, pid, net) and cgroups v2, which PRoot cannot provide.\n\
             Resolution: Start the UML backend with 'uml4termux up' and use '--runtime uml'.",
            format!("{:?}", backend.kind()).to_lowercase()
        );
    }

    let status = backend.status().await?;
    if !status.running {
        bail!("UML kernel runtime is not running. Please start it with 'uml4termux up'.");
    }

    let mut docker_cmd = vec!["docker".to_string()];
    docker_cmd.extend(args);

    let dummy_container = proot_distro_core::Container::default();
    let exit_code = backend.execute(&dummy_container, docker_cmd).await?;
    if exit_code != 0 {
        std::process::exit(exit_code);
    }
    Ok(())
}
```

---

## 4. Hardware & Software Assets Verified on Host

- **Global CLI:** `/data/data/com.termux/files/usr/bin/uml4termux` (v0.3.0+ available).
- **C Daemon Binary:** `/data/data/com.termux/files/home/workspace/uml4termux/bin/uml4termuxd`.
- **Rust Client/Daemon:** `/data/data/com.termux/files/home/workspace/uml4termux/rust/target/release/uml4termux-rs`.
- **Assets (`~/.local/share/uml4termux/`):**
  - `linux-bionic-smp` (13.2 MB, SHA256 verified): arm64 UML kernel with `OVERLAY_FS`, `NETFILTER`, `CGROUP_PIDS`, `MEMCG`.
  - `debian-docker.ext4.gz` (245 MB): Debian 12 rootfs with Docker Engine 20.10.24, containerd, runc, BuildKit.
  - `gvproxy` (12.7 MB): User-mode BESS networking and port forwarder.

---

## 5. Execution Phasing

1. **Phase 1: Runtime Abstraction Refactoring**: Create `crates/proot-distro-core/src/runtime.rs`, migrate logic to `ProotBackend`, verify existing tests.
2. **Phase 2: `UmlBackend` Client Implementation**: Wire `protocol`, `status`, `exec` socket communication and log parsing.
3. **Phase 3: `podnext docker` CLI Integration**: Enforce `backend.supports_kernel_features()` check and pass through commands to guest `dockerd`.
