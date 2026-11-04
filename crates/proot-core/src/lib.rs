//! PRoot Core - Rust Implementation
//!
//! A ptrace-based chroot, mount --bind, and binfmt_misc implementation
//! without privilege/setup for Linux. Compatible with Termux Android 16+.

/// Trace-only logging, off unless `PN_DEBUG` is set in the environment.
///
/// These lines were unconditional upstream and flooded real output — a single
/// `backup create` emitted ~280 kB of DEBUG for one alpine container, and every
/// command printed a DEBUG banner, burying the actual result. Enable with
/// `PN_DEBUG=1` when diagnosing.
macro_rules! pn_trace {
    ($($arg:tt)*) => {
        if ::std::env::var_os("PN_DEBUG").is_some() {
            ::std::eprintln!($($arg)*);
        }
    };
}
pub mod arch;
pub mod cli;
pub mod compat;
pub mod extension;
pub mod path;
pub mod ptrace;
pub mod seccomp;
pub mod syscall;
pub mod tracee;
pub mod tracee_state;
pub mod vfs;

pub use arch::{get_arch, sysnum, Arch};
pub use cli::{BindMount, ProotConfig};
pub use extension::{Extension, ExtensionManager};
pub use path::PathTranslator;
pub use ptrace::{PtraceController, PtraceEvent};
pub use seccomp::SeccompFilter;
pub use syscall::{SyscallConfig, SyscallHandler};
/// A child process exited with a code the caller should exit with itself.
///
/// Returning this as `Err` loses the code: `fn main() -> Result<()>` prints the
/// error and exits 1, so `podnext login c -- sh -c 'exit 7'` reported 1 for every
/// non-zero code. `main` downcasts to this and exits with the real one.
#[derive(Debug)]
pub struct ChildExit(pub i32);

impl std::fmt::Display for ChildExit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "child exited with code {}", self.0)
    }
}

impl std::error::Error for ChildExit {}

pub use tracee::{
    FdEntry, FdTable, MountInfo, PathInfo, RawSyscall, Registers, SyscallInfo, Tracee, TraceeId,
    TraceeState, VfsContext,
};
pub use tracee_state::TraceeStateManager;
pub use vfs::VfsManager;

use anyhow::Result;
use parking_lot::Mutex;
use std::sync::Arc;
use tokio::process::Command;

/// Resolve the proot binary to an absolute path.
///
/// Required once the child environment is cleared: Rust's Unix `Command`
/// searches a bare program name in the *child's* PATH, so the plain name
/// "proot" would no longer be found once PATH is the guest's.
fn proot_binary() -> String {
    if let Ok(explicit) = std::env::var("PROOT_BIN") {
        if !explicit.is_empty() {
            return explicit;
        }
    }
    let path = std::env::var("PATH").unwrap_or_default();
    for dir in path.split(':').filter(|d| !d.is_empty()) {
        let candidate = std::path::Path::new(dir).join("proot");
        if candidate.is_file() {
            return candidate.to_string_lossy().into_owned();
        }
    }
    "proot".to_string()
}

/// POSIX single-quote a string so an inner `/bin/sh -c` sees it as one word.
///
/// Naive `args.join(" ")` corrupts quoting: `login alpine -- /bin/sh -c 'echo X'`
/// became `/bin/sh -c /bin/sh -c echo X`, and the inner `-c` ate a word.
/// Mirrors Python's `shlex.quote`.
fn sh_quote(s: &str) -> String {
    let safe = !s.is_empty()
        && s.bytes().all(|b| {
            b.is_ascii_alphanumeric()
                || matches!(
                    b,
                    b'_' | b'-' | b'.' | b'/' | b':' | b'@' | b'%' | b'+' | b',' | b'='
                )
        });
    if safe {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// Main PRoot engine
#[derive(Clone)]
pub struct ProotEngine {
    pub config: ProotConfig,
    ptrace_ctrl: Arc<Mutex<PtraceController>>,
    syscall_handler: Arc<Mutex<SyscallHandler>>,
    path_translator: Arc<Mutex<PathTranslator>>,
    tracee_manager: Arc<Mutex<TraceeStateManager>>,
    vfs_manager: Arc<Mutex<VfsManager>>,
    seccomp_filter: Arc<Mutex<SeccompFilter>>,
}

impl ProotEngine {
    pub fn new(config: ProotConfig) -> Result<Self> {
        let ptrace_ctrl = Arc::new(Mutex::new(PtraceController::new()?));
        let syscall_config = SyscallConfig::default();
        let syscall_handler = Arc::new(Mutex::new(SyscallHandler::new(syscall_config)?));
        let path_translator = Arc::new(Mutex::new(PathTranslator::new(&config)?));
        let tracee_manager = Arc::new(Mutex::new(TraceeStateManager::new()));
        let vfs_manager = Arc::new(Mutex::new(VfsManager::new(&config)?));
        let seccomp_filter = Arc::new(Mutex::new(SeccompFilter::new(&config)?));

        Ok(Self {
            config,
            ptrace_ctrl,
            syscall_handler,
            path_translator,
            tracee_manager,
            vfs_manager,
            seccomp_filter,
        })
    }

    pub fn run(&mut self) -> Result<()> {
        // Initialize seccomp filter
        self.seccomp_filter.lock().install()?;

        // Start ptrace loop
        self.ptrace_ctrl.lock().run_loop(
            &mut self.syscall_handler.lock(),
            &mut self.path_translator.lock(),
            &mut self.tracee_manager.lock(),
            &mut self.vfs_manager.lock(),
        )
    }

    /// Add Android/Termux system binds required for dynamic linker to work
    fn add_android_system_binds(cmd: &mut Command, isolated: bool) {
        // Android system directories - required for dynamic linker
        cmd.arg("--bind=/apex");
        cmd.arg("--bind=/odm");
        cmd.arg("--bind=/product");
        cmd.arg("--bind=/system");
        cmd.arg("--bind=/system_ext");
        cmd.arg("--bind=/vendor");

        // Linker config - required for dynamic linker
        cmd.arg("--bind=/linkerconfig/ld.config.txt");
        cmd.arg("--bind=/linkerconfig/com.android.art/ld.config.txt");

        // In isolated mode, skip all host user storage, home directories, and caches
        if isolated {
            return;
        }

        // Termux prefix - required for Termux utilities
        if let Ok(prefix) = std::env::var("TERMUX_PREFIX") {
            cmd.arg(format!("--bind={}", prefix));
        } else {
            cmd.arg("--bind=/data/data/com.termux/files/usr");
        }

        // Android dalvik/ART caches
        for cache_dir in &[
            "/data/app",
            "/data/dalvik-cache",
            "/data/misc/apexdata/com.android.art/dalvik-cache",
        ] {
            if std::path::Path::new(cache_dir).exists() {
                cmd.arg(format!("--bind={}", cache_dir));
            }
        }

        // Storage binds (if available)
        for storage_dir in &["/storage/self/primary", "/sdcard", "/storage/emulated/0"] {
            if std::path::Path::new(storage_dir).exists() {
                cmd.arg(format!("--bind={}", storage_dir));
            }
        }

        // Termux app directories
        if let Ok(home) = std::env::var("HOME") {
            cmd.arg(format!("--bind={}", home));
        }
        if let Ok(cache) = std::env::var("TERMUX_PREFIX") {
            let termux_cache = format!("{}/tmp", cache);
            if std::path::Path::new(&termux_cache).exists() {
                cmd.arg(format!("--bind={}", termux_cache));
            }
        }
    }

    /// Start a daemon process (background mode)
    pub async fn start(&mut self) -> Result<u32> {
        // Initialize seccomp filter
        self.seccomp_filter.lock().install()?;

        // Build the proot command for proper isolation
        // Note: Use only options supported by the system proot binary (5.1.107.101)
        // IMPORTANT: proot requires --rootfs=. with cwd set to rootfs for proper isolation
        let proot_bin = proot_binary();
        let mut cmd = Command::new(&proot_bin);

        // Proot extensions for Termux/Android (supported by this proot version)
        cmd.arg("--kill-on-exit");
        cmd.arg("--link2symlink");
        cmd.arg("--sysvipc");
        cmd.arg("-L"); // Fix lstat for dpkg symlink warnings

        // Present uid/gid 0 inside the guest.
        //
        // Without this the guest keeps the real Android app uid and every
        // privileged operation fails at the first check: `apt-get install`
        // aborts with `dpkg: error: requested operation requires superuser
        // privilege`, and `chown`/`passwd`/`useradd` refuse too, which is what
        // blocked node, playwright and any package install in a Debian
        // container. proot-distro passes exactly this for non-termux
        // containers (proot_cmd.py:105, `--change-id={login_uid}:{login_gid}`,
        // defaulting to 0:0).
        //
        // It is still a user-space illusion: proot maps ids for the traced
        // process only, no real privilege is gained.
        cmd.arg("--change-id=0:0");

        // Root filesystem - critical for isolation
        // Use --rootfs=. with cwd set to rootfs (like proot-distro does)
        let rootfs = self
            .config
            .rootfs
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Rootfs not configured"))?;
        cmd.arg("--rootfs=.");
        cmd.current_dir(rootfs);

        // Working directory inside guest rootfs
        if let Some(cwd) = &self.config.cwd {
            cmd.arg(format!("--cwd={}", cwd.display()));
        }

        // Essential virtual filesystem binds for isolation
        cmd.arg("--bind=/dev");
        cmd.arg("--bind=/proc");
        cmd.arg("--bind=/sys");

        // Android/Termux system binds required for dynamic linker
        Self::add_android_system_binds(&mut cmd, self.config.isolated);

        // Environment variables — allow-listed guest env (see execute()).
        cmd.env_clear();
        cmd.env(
            "PATH",
            "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
        );
        cmd.env("HOME", "/root");
        cmd.env("TERM", "xterm-256color");
        for passthrough in ["LANG", "LC_ALL", "COLORTERM"] {
            if let Ok(v) = std::env::var(passthrough) {
                cmd.env(passthrough, v);
            }
        }
        for env in &self.config.env {
            if let Some((k, v)) = env.split_once('=') {
                cmd.env(k, v);
            }
        }

        // Additional bind mounts from config
        for bind in &self.config.bind {
            cmd.arg(format!("--bind={}", bind));
        }

        // Command to execute
        if self.config.command.is_empty() {
            return Err(anyhow::anyhow!("No command specified"));
        }
        cmd.arg(&self.config.command[0]);
        if self.config.command.len() > 1 {
            cmd.args(&self.config.command[1..]);
        }

        // Debug output
        if self.config.verbose > 0 {
            pn_trace!("[DEBUG] Starting proot command: {:?}", cmd);
            pn_trace!("[DEBUG] Rootfs: {:?}", rootfs);
            pn_trace!("[DEBUG] Env: {:?}", self.config.env);
        }

        // Own process group: proot traces the guest's descendants, so a stop
        // must be able to signal the whole tree, not one pid. pgid == child pid.
        cmd.process_group(0);
        // Spawn the process (daemon mode)
        let child = cmd.spawn()?;
        let pid = child
            .id()
            .ok_or_else(|| anyhow::anyhow!("Failed to get PID"))?;

        if self.config.verbose > 0 {
            pn_trace!("[DEBUG] Started daemon PID: {}", pid);
        }

        Ok(pid)
    }

    /// Execute a command in the foreground and wait for completion, returning exit code
    pub async fn execute(&mut self) -> Result<i32> {
        // Initialize seccomp filter
        self.seccomp_filter.lock().install()?;

        // Build the proot command for proper isolation
        // Note: Use only options supported by the system proot binary (5.1.107.101)
        // IMPORTANT: proot requires --rootfs=. with cwd set to rootfs for proper isolation
        // Also, run commands through the shell like proot-distro does
        let proot_bin = proot_binary();
        let mut cmd = Command::new(&proot_bin);

        // Proot extensions for Termux/Android (supported by this proot version)
        cmd.arg("--kill-on-exit");
        cmd.arg("--link2symlink");
        cmd.arg("--sysvipc");
        cmd.arg("-L"); // Fix lstat for dpkg symlink warnings

        // Present uid/gid 0 inside the guest.
        //
        // Without this the guest keeps the real Android app uid and every
        // privileged operation fails at the first check: `apt-get install`
        // aborts with `dpkg: error: requested operation requires superuser
        // privilege`, and `chown`/`passwd`/`useradd` refuse too, which is what
        // blocked node, playwright and any package install in a Debian
        // container. proot-distro passes exactly this for non-termux
        // containers (proot_cmd.py:105, `--change-id={login_uid}:{login_gid}`,
        // defaulting to 0:0).
        //
        // It is still a user-space illusion: proot maps ids for the traced
        // process only, no real privilege is gained.
        cmd.arg("--change-id=0:0");

        // Root filesystem - critical for isolation
        // Use --rootfs=. with cwd set to rootfs (like proot-distro does)
        let rootfs = self
            .config
            .rootfs
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Rootfs not configured"))?;
        cmd.arg("--rootfs=.");
        cmd.current_dir(rootfs);

        // Working directory inside guest rootfs
        if let Some(cwd) = &self.config.cwd {
            cmd.arg(format!("--cwd={}", cwd.display()));
        }

        // Essential virtual filesystem binds for isolation
        cmd.arg("--bind=/dev");
        cmd.arg("--bind=/proc");
        cmd.arg("--bind=/sys");

        // Android/Termux system binds required for dynamic linker
        Self::add_android_system_binds(&mut cmd, self.config.isolated);

        // Environment variables
        //
        // The container gets a clean, allow-listed environment — not the host's.
        // Passing the host env through (the previous behaviour) leaked live
        // secrets (`*_API_KEY`) into the guest and broke every musl binary:
        // Termux's LD_PRELOAD (libtermux-exec) and LD_LIBRARY_PATH are host-ABI
        // and cause "relocation error" + exit 127 for apk/busybox/getent.
        // proot-distro builds the guest env from scratch (env -i) for the same
        // reasons. Verified: after this, `login alpine -- apk --version` prints
        // "apk-tools 3.0.8-r0" and the guest no longer sees host secrets.
        cmd.env_clear();
        cmd.env(
            "PATH",
            "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
        );
        cmd.env("HOME", "/root");
        cmd.env("TERM", "xterm-256color");
        for passthrough in ["LANG", "LC_ALL", "COLORTERM"] {
            if let Ok(v) = std::env::var(passthrough) {
                cmd.env(passthrough, v);
            }
        }
        for env in &self.config.env {
            if let Some((k, v)) = env.split_once('=') {
                cmd.env(k, v);
            }
        }

        // Additional bind mounts from config
        for bind in &self.config.bind {
            cmd.arg(format!("--bind={}", bind));
        }

        // Command to execute - run through shell like proot-distro does
        // This ensures PATH resolution and symlink handling works correctly.
        // Each argv element is quoted individually so the inner shell sees
        // exactly the words the caller passed.
        if self.config.command.is_empty() {
            return Err(anyhow::anyhow!("No command specified"));
        }
        let shell = "/bin/sh";
        let command_str = self
            .config
            .command
            .iter()
            .map(|a| sh_quote(a))
            .collect::<Vec<_>>()
            .join(" ");
        cmd.arg(shell);
        cmd.arg("-c");
        cmd.arg(command_str);

        // Debug output
        if self.config.verbose > 0 {
            pn_trace!("[DEBUG] Executing proot command: {:?}", cmd);
            pn_trace!("[DEBUG] Rootfs: {:?}", rootfs);
            pn_trace!("[DEBUG] Env: {:?}", self.config.env);
        }

        // Spawn and wait for completion
        let mut child = cmd
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .stdin(std::process::Stdio::inherit())
            // Own process group (pgid == this pid) so callers can group-signal.
            .process_group(0)
            .spawn()?;
        let pid = child
            .id()
            .ok_or_else(|| anyhow::anyhow!("Failed to get PID"))?;
        if self.config.verbose > 0 {
            pn_trace!("[DEBUG] Spawned PID: {}", pid);
        }

        let exit_status = child.wait().await?;
        if self.config.verbose > 0 {
            pn_trace!("[DEBUG] Exit status: {:?}", exit_status);
        }
        let exit_code = exit_status.code().unwrap_or(-1);
        if self.config.verbose > 0 {
            pn_trace!("[DEBUG] Exit code: {}", exit_code);
        }

        Ok(exit_code)
    }

    pub fn shutdown(&mut self) -> Result<()> {
        self.ptrace_ctrl.lock().shutdown()
    }
}

/// Initialize PRoot core with default configuration
pub fn init() -> Result<ProotEngine> {
    let config = ProotConfig::default();
    ProotEngine::new(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init() {
        let engine = init();
        assert!(engine.is_ok());
    }

    #[test]
    fn test_proot_engine_new() {
        let config = ProotConfig::default();
        let engine = ProotEngine::new(config);
        assert!(engine.is_ok());
    }

    #[test]
    fn test_proot_config_default() {
        let config = ProotConfig::default();
        assert!(config.command.is_empty());
        assert!(config.bind.is_empty());
        assert!(config.env.is_empty());
    }

    #[test]
    fn test_proot_config_custom() {
        let config = ProotConfig {
            command: vec!["/bin/bash".to_string()],
            bind: vec!["/host:/guest".to_string()],
            env: vec!["TEST=value".to_string()],
            ..Default::default()
        };
        assert_eq!(config.command, vec!["/bin/bash".to_string()]);
        assert_eq!(config.bind, vec!["/host:/guest".to_string()]);
        assert_eq!(config.env, vec!["TEST=value".to_string()]);
    }
}
