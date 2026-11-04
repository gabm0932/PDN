//! CLI configuration for PRoot

use clap::Parser;
use std::collections::HashMap;
use std::path::PathBuf;

/// PRoot CLI configuration
#[derive(Parser, Debug, Clone)]
#[command(name = "proot")]
#[command(about = "PRoot - chroot, mount --bind, and binfmt_misc without privilege/setup")]
pub struct ProotConfig {
    /// Root filesystem path
    #[arg(short = 'r', long = "rootfs")]
    pub rootfs: Option<PathBuf>,

    /// Bind mounts (source:target[:readonly])
    #[arg(short = 'b', long = "bind")]
    pub bind: Vec<String>,

    /// Working directory
    #[arg(short = 'w', long = "cwd")]
    pub cwd: Option<PathBuf>,

    /// User ID to run as
    #[arg(long = "uid")]
    pub uid: Option<u32>,

    /// Group ID to run as
    #[arg(long = "gid")]
    pub gid: Option<u32>,

    /// Environment variables
    #[arg(long = "env")]
    pub env: Vec<String>,

    /// Enable seccomp filtering
    #[arg(long = "seccomp")]
    pub seccomp: bool,

    /// Enable syscall tracing
    #[arg(long = "trace")]
    pub trace: bool,

    /// Kernel release to report
    #[arg(long = "kernel-release")]
    pub kernel_release: Option<String>,

    /// Hostname to set
    #[arg(long = "hostname")]
    pub hostname: Option<String>,

    /// Kill on exit
    #[arg(long = "kill-on-exit")]
    pub kill_on_exit: bool,

    /// Link to symlink
    #[arg(long = "link2symlink")]
    pub link2symlink: bool,

    /// SysV IPC
    #[arg(long = "sysvipc")]
    pub sysvipc: bool,

    /// Isolated mode (do not bind host storage, dalvik cache, or host home directory)
    #[arg(long = "isolated")]
    pub isolated: bool,

    /// Verbose output
    #[arg(short = 'v', long = "verbose", action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Command to run
    #[arg(last = true)]
    pub command: Vec<String>,
}

impl Default for ProotConfig {
    fn default() -> Self {
        Self {
            rootfs: None,
            bind: Vec::new(),
            cwd: None,
            uid: None,
            gid: None,
            env: Vec::new(),
            seccomp: true,
            trace: false,
            kernel_release: None,
            hostname: None,
            kill_on_exit: true,
            link2symlink: true,
            sysvipc: true,
            isolated: false,
            verbose: 0,
            command: Vec::new(),
        }
    }
}

impl ProotConfig {
    pub fn parse_bind_mounts(&self) -> Vec<BindMount> {
        self.bind
            .iter()
            .filter_map(|s| {
                let parts: Vec<&str> = s.split(':').collect();
                if parts.len() >= 2 {
                    Some(BindMount {
                        source: PathBuf::from(parts[0]),
                        target: PathBuf::from(parts[1]),
                        readonly: parts.get(2).map(|s| *s == "ro").unwrap_or(false),
                    })
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn parse_env(&self) -> HashMap<String, String> {
        self.env
            .iter()
            .filter_map(|s| {
                let parts: Vec<&str> = s.splitn(2, '=').collect();
                if parts.len() == 2 {
                    Some((parts[0].to_string(), parts[1].to_string()))
                } else {
                    None
                }
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct BindMount {
    pub source: std::path::PathBuf,
    pub target: std::path::PathBuf,
    pub readonly: bool,
}
