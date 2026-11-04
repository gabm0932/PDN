//! podnext CLI - Advanced container and service management for Termux/Android
//!
//! A comprehensive CLI that works with proot-distro-core, pn-next, and pn-gateway
//! to provide full container lifecycle management, service orchestration, and monitoring.

#![allow(deprecated)]
#![allow(future_incompatible)]

/// Trace-only logging, off unless `PN_DEBUG` is set in the environment.
///
/// These lines were unconditional upstream and flooded real output — a single
/// `backup create` emitted ~280 kB of DEBUG for one alpine container, burying
/// the result. Enable with `PN_DEBUG=1` when diagnosing.
macro_rules! pn_trace {
    ($($arg:tt)*) => {
        if ::std::env::var_os("PN_DEBUG").is_some() {
            ::std::eprintln!($($arg)*);
        }
    };
}
use anyhow::{bail, Result};
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use serde::Serialize;
use serde_yaml;
use std::io::{self, Write};
use std::path::PathBuf;
use tabled::{Table, Tabled};
use tracing_subscriber::EnvFilter;

mod commands;
mod native;
mod utils;

use native::{
    install_image_native, kill_container_native, list_containers_native, list_sessions_native,
    list_sessions_native_full, login_container_native, remove_container_native,
    run_container_native, search_docker_hub, verify_image_available,
};
use utils::format_size;

#[derive(Serialize, Clone)]
struct SerializableInfo {
    version: String,
    platform: String,
    rust_version: String,
    backend: String,
}

#[derive(Serialize, Clone)]
struct SerializableImage {
    repo_name: String,
    short_description: String,
    star_count: u32,
    pull_count: u64,
    is_official: bool,
    is_automated: bool,
}

use commands::*;
use proot_distro_core::network::NetworkConfig;
use proot_distro_core::storage::VolumeConfig;

#[derive(Parser)]
#[command(name = "podnext")]
#[command(
    about = "PRoot Distro Next (PDN) - Fast, isolated container and service orchestration for Termux/Android"
)]
#[command(version)]
#[command(
    long_about = "podnext (PRoot Distro Next) - Next-generation container & service orchestration platform.\n\n\
    ========================================================================\n\
    1. SIMPLE & CORE DISTRO USAGE (Pengganti proot-distro sehari-hari):\n\
    ========================================================================\n\
    podnext install debian:13                 # Pasang distro Debian 13 trixie\n\
    podnext login debian                      # Masuk ke shell interaktif container\n\
    podnext run debian -- apt update          # Jalankan perintah langsung di container\n\
    podnext list                              # Tampilkan daftar container terpasang\n\
    podnext copy debian:/etc/hosts ./hosts    # Salin berkas/folder host <-> container\n\
    podnext sync ./src debian:/root/src -d    # Sinkronisasi direktori (hashing sha256 + prune)\n\
    podnext rename debian deb-prod            # Ganti nama container terpasang\n\
    podnext reset deb-prod                    # Reset container ke instalasi bersih semula\n\
    podnext remove deb-prod                   # Hapus container beserta datanya\n\
    podnext search debian                     # Cari OCI image di Docker Hub\n\
    podnext info                              # Tampilkan informasi sistem & host kernel\n\n\
    ========================================================================\n\
    2. INTERMEDIATE: IMAGES, STORAGE, VOLUMES & BACKUP:\n\
    ========================================================================\n\
    podnext image ls                          # Tampilkan cache image lokal\n\
    podnext image pull alpine:latest          # Unduh image OCI dari registry\n\
    podnext volume create my-vol              # Buat volume penyimpanan persisten\n\
    podnext volume ls                         # Tampilkan daftar volume\n\
    podnext volume prune                      # Bersihkan volume tak terpakai\n\
    podnext network create isolated-net       # Buat jaringan bridge terisolasi\n\
    podnext network ls                        # Tampilkan jaringan container\n\
    podnext backup create debian              # Buat arsip snapshot container (tar.gz)\n\
    podnext backup list                       # Tampilkan daftar snapshot cadangan\n\
    podnext backup restore <id> --target deb2 # Pulihkan container dari backup\n\n\
    ========================================================================\n\
    3. ADVANCED: SYSTEM SERVICES, DAEMON & SUPERVISOR:\n\
    ========================================================================\n\
    podnext daemon start                      # Jalankan supervisor daemon PDN\n\
    podnext daemon status                     # Periksa status daemon supervisor\n\
    podnext service start web-srv             # Jalankan managed service (dengan restart policy)\n\
    podnext service status web-srv            # Tampilkan status & PID service\n\
    podnext service logs web-srv              # Tampilkan log service\n\
    podnext quota set web-srv memory 512M     # Terapkan kuota cgroup memory\n\
    podnext quota set web-srv cpu 200         # Terapkan batas kuota CPU (2 cores)\n\
    podnext quota usage web-srv               # Pantau utilisasi resource real-time\n\
    podnext watchdog status                   # Pantau circuit breaker & watchdog\n\
    podnext log tail web-srv                  # Streaming log secara real-time\n\
    podnext dbus serve --socket /tmp/d.sock   # Jalankan D-Bus broker IPC\n\
    podnext dbus call ...                     # Kirim panggilan method D-Bus\n\
    podnext gitops sync                       # Sinkronisasi deklaratif GitOps manifests\n\
    podnext gitops status                     # Status sinkronisasi aplikasi GitOps\n\
    podnext boot enable web-srv               # Pasang auto-start saat Termux boot\n\
    podnext wakelock acquire web-srv Partial  # Ambil Android wake lock agar tetap aktif\n\n\
    ========================================================================\n\
    4. DIRECT EXECUTION MODE (Kompatibilitas murni seperti biner proot):\n\
    ========================================================================\n\
    podnext -r /path/to/rootfs -- /bin/bash    # Jalankan rootfs kustom secara langsung\n\
    podnext -b /host/dir:/guest/dir -- ls      # Bind mount direktori host langsung\n"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    // Direct execution mode (like proot)
    #[arg(
        short = 'r',
        long = "rootfs",
        global = true,
        help = "Use PATH as guest root filesystem"
    )]
    rootfs: Option<String>,

    #[arg(
        short = 'b',
        long = "bind",
        global = true,
        value_name = "HOST_PATH:GUEST_PATH",
        help = "Bind mount host path into guest (repeatable)"
    )]
    bind: Vec<String>,

    #[arg(
        short = 'w',
        long = "pwd",
        global = true,
        help = "Set initial working directory"
    )]
    pwd: Option<String>,

    #[arg(
        short = 'q',
        long = "qemu",
        global = true,
        help = "Execute through QEMU user-mode emulation (path to qemu binary)"
    )]
    qemu: Option<String>,

    #[arg(
        short = '0',
        long = "link2symlink",
        global = true,
        help = "Enable hardlink emulation via symlinks"
    )]
    link2symlink: bool,

    #[arg(
        long = "no-sysvipc",
        global = true,
        help = "Disable System V IPC emulation"
    )]
    no_sysvipc: bool,

    #[arg(
        long = "sysvipc",
        global = true,
        help = "Enable System V IPC emulation"
    )]
    sysvipc: bool,

    #[arg(
        long = "kernel",
        global = true,
        help = "Override kernel release string"
    )]
    kernel: Option<String>,

    #[arg(long = "hostname", global = true, help = "Set hostname")]
    hostname: Option<String>,

    #[arg(long = "uid", global = true, help = "User ID to run as")]
    uid: Option<u32>,

    #[arg(long = "gid", global = true, help = "Group ID to run as")]
    gid: Option<u32>,

    #[arg(long = "seccomp", global = true, help = "Enable seccomp filtering")]
    seccomp: bool,

    #[arg(long = "no-seccomp", global = true, help = "Disable seccomp filtering")]
    no_seccomp: bool,

    #[arg(long = "trace", global = true, help = "Enable syscall tracing")]
    trace: bool,

    #[arg(long = "kill-on-exit", global = true, help = "Kill processes on exit")]
    kill_on_exit: bool,

    #[arg(
        long = "no-kill-on-exit",
        global = true,
        help = "Don't kill processes on exit"
    )]
    no_kill_on_exit: bool,

    #[arg(
        short = 'e',
        long = "env",
        global = true,
        value_name = "VAR=VALUE",
        help = "Set environment variable (repeatable)"
    )]
    env: Vec<String>,

    /// Enable verbose debug output
    #[arg(short = 'v', long = "verbose", global = true, action = clap::ArgAction::Count, help = "Enable verbose debug output (repeatable)")]
    verbose: u8,
}

#[derive(Subcommand, Debug)]
#[command(rename_all = "lower")]
enum Commands {
    // ------------------------------------------------------------------------
    // TIER 1: CORE DISTRO COMMANDS (Simple & Daily Usage - proot-distro parity)
    // ------------------------------------------------------------------------
    /// [Core] Install distribution from OCI image
    #[command(
        long_about = "Download and unpack an OCI/Docker container image into a clean, isolated rootfs.\n\n\
        EXAMPLES:\n  \
        podnext install debian:13\n  \
        podnext install ubuntu:22.04 --name my-ubuntu\n  \
        podnext install alpine:latest --force"
    )]
    Install {
        /// Image reference (e.g., ubuntu:22.04, debian:12, alpine:latest)
        image: String,
        /// Container name (defaults to image name without tag)
        #[arg(short, long)]
        name: Option<String>,
        /// Force reinstall if container exists
        #[arg(short, long)]
        force: bool,
    },

    /// [Core] List created containers
    #[command(
        long_about = "List all installed distributions and containers with their status, real Linux OS, and architecture.\n\n\
        EXAMPLES:\n  \
        podnext list\n  \
        podnext list --format json\n  \
        podnext list --no-all"
    )]
    List {
        /// Show all (including stopped)
        #[arg(short = 'a', long, default_value = "true")]
        all: bool,
        /// Hide stopped containers
        #[arg(long = "no-all", action = clap::ArgAction::SetTrue)]
        no_all: bool,
        /// Output format
        #[arg(short, long, value_enum, default_value = "table")]
        format: OutputFormat,
    },

    /// [Core] Start interactive shell inside a container
    #[command(
        long_about = "Start an interactive root/user shell inside an installed container with isolated environment.\n\n\
        EXAMPLES:\n  \
        podnext login debian\n  \
        podnext login ubuntu -- /bin/bash\n  \
        podnext login debian --user 1000"
    )]
    Login {
        /// Container name
        container: String,
        /// User to run as
        #[arg(short, long)]
        user: Option<String>,
        /// Run in isolated mode (do not mount host storage or Termux home directory)
        #[arg(long = "isolated")]
        isolated: bool,
        /// Command to run instead of shell (after --)
        #[arg(last = true)]
        command: Vec<String>,
    },

    /// [Core] Run command in container
    #[command(
        long_about = "Run a command inside an installed container directly from the host without opening an interactive shell.\n\n\
        EXAMPLES:\n  \
        podnext run debian -- apt update && apt upgrade -y\n  \
        podnext run debian -- curl -fsSL https://example.com\n  \
        podnext run ubuntu -v /sdcard:/mnt/sdcard -- ls -la /mnt/sdcard"
    )]
    Run {
        /// Container name
        container: String,
        /// Command to run (after --)
        #[arg(last = true)]
        command: Vec<String>,
        /// Automatically remove the container when it exits
        #[arg(long = "rm", global = false)]
        rm: bool,
        /// Run container in background and print container ID
        #[arg(long = "detach", short = 'd', global = false)]
        detach: bool,
        /// Assign a name to the container
        #[arg(long = "name", global = false)]
        name: Option<String>,
        /// Connect a container to a network
        #[arg(long = "network", global = false)]
        network: Option<String>,
        /// Bind mount a volume (host_path:container_path[:ro])
        #[arg(
            long = "volume",
            global = false,
            value_name = "HOST_PATH:CONTAINER_PATH[:ro]"
        )]
        volume: Vec<String>,
        /// Publish a container's port(s) to the host (host_port:container_port[/protocol])
        #[arg(
            long = "publish",
            short = 'p',
            global = false,
            value_name = "HOST_PORT:CONTAINER_PORT[/PROTOCOL]"
        )]
        publish: Vec<String>,
        /// Overwrite the default ENTRYPOINT of the image
        #[arg(long = "entrypoint", global = false)]
        entrypoint: Option<String>,
        /// Allocate a pseudo-TTY
        #[arg(long = "tty", short = 't', global = false)]
        tty: bool,
        /// Keep STDIN open even if not attached
        #[arg(long = "interactive", short = 'i', global = false)]
        interactive: bool,
        /// User to run as (format: <name|uid>[:<group|gid>])
        #[arg(long = "user", short = 'u', global = false)]
        user: Option<String>,
        /// Working directory inside the container
        #[arg(long = "workdir", global = false)]
        workdir: Option<String>,
        /// Run in isolated mode (do not mount host storage or Termux home directory)
        #[arg(long = "isolated", global = false)]
        isolated: bool,
    },

    /// [Container] List active sessions
    Ps {
        /// Output format
        #[arg(short, long, value_enum, default_value = "table")]
        format: OutputFormat,
    },

    /// [Container] Stop container
    Kill {
        /// Container name
        container: String,
    },

    /// [Core] Delete a container
    #[command(
        long_about = "Permanently remove an installed container and its rootfs from disk.\n\n\
        EXAMPLES:\n  \
        podnext remove debian\n  \
        podnext remove my-ubuntu --force"
    )]
    Remove {
        /// Container name
        name: String,
        /// Force remove without confirmation
        #[arg(short = 'f', long)]
        force: bool,
    },

    /// [Core] Rename an installed container
    #[command(
        long_about = "Safely rename an existing container in its directory structure and metadata.\n\n\
        EXAMPLES:\n  \
        podnext rename debian deb-prod\n  \
        podnext rename ubuntu test-ub"
    )]
    Rename {
        /// Current container name
        oldname: String,
        /// New container name
        newname: String,
    },

    /// [Core] Copy files or directories between host and containers
    ///
    /// Paths are `[CONTAINER:]PATH`; a path without a container prefix is the
    /// host's.
    #[command(
        long_about = "Copy files or directories between the host and containers safely (strictly rejects path traversal escapes).\n\n\
        EXAMPLES:\n  \
        podnext copy debian:/etc/resolv.conf ./resolv.conf\n  \
        podnext copy ./my-app debian:/opt/my-app -R\n  \
        podnext copy debian:/var/log/app.log ./app.log -m"
    )]
    Copy {
        /// `[CONTAINER:]SRC`
        source: String,
        /// `[CONTAINER:]DEST`
        destination: String,
        /// Copy directories recursively (`-R`, since `-r` is the global --rootfs)
        #[arg(short = 'R', long)]
        recursive: bool,
        /// Remove the source after a successful copy
        #[arg(short = 'm', long = "move")]
        move_: bool,
    },

    /// [Core] Reset a container to a clean install of the image it came from
    ///
    /// Deletes the container directory and reinstalls it. All data inside the
    /// container is lost, as with proot-distro's `reset`.
    #[command(
        long_about = "Reinstall a container cleanly from its original OCI image, discarding all user modifications.\n\n\
        EXAMPLES:\n  \
        podnext reset debian"
    )]
    Reset {
        /// Container name
        name: String,
    },

    /// [Core] Mirror files between host and containers (content hashing sha256)
    ///
    /// Like `copy`, but only the differences move: a file is transferred when its
    /// content differs, so a second run is a no-op. Paths are `[CONTAINER:]PATH`.
    #[command(
        long_about = "Fast delta directory synchronization between host and containers based on SHA256 content hashing.\n\n\
        EXAMPLES:\n  \
        podnext sync ./src debian:/root/src\n  \
        podnext sync ./src debian:/root/src --delete"
    )]
    Sync {
        /// `[CONTAINER:]SRC`
        source: String,
        /// `[CONTAINER:]DEST`
        destination: String,
        /// Remove destination entries that have no counterpart in the source
        #[arg(short = 'd', long)]
        delete: bool,
    },

    /// [Core] Search images on Docker Hub
    #[command(
        long_about = "Search for container images on Docker Hub, or list verified official distributions when run without arguments.\n\n\
        EXAMPLES:\n  \
        podnext search\n  \
        podnext search debian\n  \
        podnext search ubuntu --limit 5\n  \
        podnext search alpine --format json"
    )]
    Search {
        /// Search query (omit to display popular verified distributions)
        #[arg(default_value = "")]
        query: String,
        /// Limit results
        #[arg(short, long, default_value = "10")]
        limit: usize,
        /// Output format
        #[arg(short, long, value_enum, default_value = "table")]
        format: OutputFormat,
    },

    /// [Container] Manage OCI images and local cache
    #[command(subcommand)]
    Image(ImageCommands),

    /// [Container] List active sessions (detailed)
    Sessions {
        /// Output format
        #[arg(short, long, value_enum, default_value = "table")]
        format: OutputFormat,
    },

    /// [Container] Attach to a container session (creates new session)
    Attach {
        /// Container name
        container: String,
        /// User to run as
        #[arg(short, long)]
        user: Option<String>,
        /// Command to run instead of shell (after --)
        command: Vec<String>,
    },

    /// [Core] Show system information
    #[command(name = "info")]
    #[command(
        long_about = "Display host kernel, system architecture, CPU cores, memory limits, and PDN capabilities.\n\n\
        EXAMPLES:\n  \
        podnext info\n  \
        podnext info --format json"
    )]
    Info {
        /// Output format
        #[arg(short, long, value_enum, default_value = "table")]
        format: OutputFormat,
    },

    /// [Core] Show version
    #[command(name = "version")]
    #[command(long_about = "Display version and build information for PDN.\n\n\
        EXAMPLES:\n  \
        podnext version\n  \
        podnext version --format json")]
    Version {
        /// Output format
        #[arg(short, long, value_enum, default_value = "text")]
        format: OutputFormat,
    },

    // ------------------------------------------------------------------------
    // TIER 2 & 3: ADVANCED SYSTEM SERVICES & ORCHESTRATION
    // ------------------------------------------------------------------------
    /// [System] Service management commands (supervision & lifecycle)
    #[command(subcommand)]
    Service(ServiceCommands),

    /// [System] Daemon management (background supervisor process)
    #[command(subcommand)]
    Daemon(DaemonCommands),

    /// [System] Watchdog management & circuit breaker
    #[command(subcommand)]
    Watchdog(WatchdogCommands),

    /// [System] Resource quota management (memory/cpu cgroups)
    #[command(subcommand)]
    Quota(QuotaCommands),

    /// [System] Structured logging & real-time streaming
    #[command(subcommand)]
    Log(LogCommands),

    /// [System] D-Bus IPC & service brokering
    #[command(subcommand)]
    Dbus(DbusCommands),

    /// [System] Socket activation (listen on host, wake container on demand)
    #[command(subcommand)]
    Socket(SocketCommands),

    /// [System] Termux:Boot integration for background autostart
    #[command(subcommand)]
    Boot(BootCommands),

    /// [System] Wake lock management (prevent Android sleep)
    #[command(subcommand)]
    Wakelock(WakelockCommands),

    /// [System] Policy engine (RBAC & capability limits)
    #[command(subcommand)]
    Policy(PolicyCommands),

    /// [Storage] Backup/Restore management (container snapshots)
    #[command(subcommand)]
    Backup(BackupCommands),

    /// [Advanced] Multi-arch build service
    #[command(subcommand)]
    Build(BuildCommands),

    /// [System] GitOps integration (declarative YAML sync & reconciliation)
    #[command(subcommand)]
    Gitops(GitopsCommands),

    /// [System] Web dashboard management
    #[command(subcommand)]
    Dashboard(DashboardCommands),

    /// [Storage] Network management (isolated bridge networks)
    #[command(subcommand)]
    Network(NetworkCommands),

    /// [Storage] Volume management (persistent data volumes)
    #[command(subcommand)]
    Volume(VolumeCommands),

    /// [Advanced] Run Docker CLI commands transparently via kernel-capable runtime (UML)
    #[command(
        long_about = "Execute Docker CLI commands transparently through User-Mode Linux (UML) runtime.\n\n\
        EXAMPLES:\n  \
        podnext docker run --rm hello-world\n  \
        podnext docker ps\n  \
        podnext docker images"
    )]
    Docker {
        /// Optional container or instance name
        #[arg(short, long)]
        container: Option<String>,

        /// Arguments passed through to the docker CLI
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// [Core] Generate shell completions
    Completion {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },
}

#[derive(Subcommand, Debug)]
#[command(rename_all = "lower")]
enum ImageCommands {
    /// Pull an image from a registry
    Pull {
        /// Image reference (e.g., ubuntu:22.04, alpine:latest)
        image: String,
        /// Platform (e.g., linux/arm64, linux/amd64)
        #[arg(long)]
        platform: Option<String>,
        /// Download all tagged images
        #[arg(long)]
        all_tags: bool,
    },
    /// Push an image to a registry
    Push {
        /// Image reference (e.g., ubuntu:22.04)
        image: String,
    },
    /// Remove one or more images
    Rm {
        /// Image references to remove
        images: Vec<String>,
        /// Force removal without confirmation
        #[arg(short = 'f', long)]
        force: bool,
    },
    /// Tag an image
    Tag {
        /// Source image reference
        source: String,
        /// Target image reference
        target: String,
    },
    /// Show image history
    History {
        /// Image reference
        image: String,
        /// Show full history without truncation
        #[arg(long)]
        no_trunc: bool,
        /// Output format
        #[arg(short, long, value_enum, default_value = "table")]
        format: OutputFormat,
    },
    /// Display detailed information on one or more images
    Inspect {
        /// Image references to inspect
        images: Vec<String>,
        /// Output format
        #[arg(short, long, value_enum, default_value = "json")]
        format: OutputFormat,
    },
    /// List images
    Ls {
        /// Output format
        #[arg(short, long, value_enum, default_value = "table")]
        format: OutputFormat,
        /// Show all images (including intermediate)
        #[arg(short = 'a', long)]
        all: bool,
    },
}

#[derive(Subcommand, Debug)]
#[command(rename_all = "lower")]
enum NetworkCommands {
    /// Create a network
    Create {
        /// Network name
        name: String,
        /// Network driver
        #[arg(short = 'd', long, default_value = "bridge")]
        driver: String,
        /// Network subnet (CIDR)
        #[arg(short = 's', long)]
        subnet: Option<String>,
        /// Network gateway
        #[arg(short = 'g', long)]
        gateway: Option<String>,
    },
    /// List networks
    Ls {
        /// Output format
        #[arg(short, long, value_enum, default_value = "table")]
        format: OutputFormat,
    },
    /// Inspect a network
    Inspect {
        /// Network name(s)
        networks: Vec<String>,
        /// Output format
        #[arg(short, long, value_enum, default_value = "json")]
        format: OutputFormat,
    },
    /// Remove one or more networks
    Rm {
        /// Network name(s)
        networks: Vec<String>,
    },
    /// Connect a container to a network
    Connect {
        /// Network name
        network: String,
        /// Container name
        container: String,
        /// Container IP address
        #[arg(short = 'i', long)]
        ip: Option<String>,
    },
    /// Disconnect a container from a network
    Disconnect {
        /// Network name
        network: String,
        /// Container name
        container: String,
    },
}

#[derive(Subcommand, Debug)]
#[command(rename_all = "lower")]
enum VolumeCommands {
    /// Create a volume
    Create {
        /// Volume name
        name: String,
        /// Volume driver
        #[arg(short = 'd', long, default_value = "local")]
        driver: String,
        /// Driver options
        #[arg(short = 'o', long, value_name = "KEY=VALUE")]
        opts: Vec<String>,
    },
    /// List volumes
    Ls {
        /// Output format
        #[arg(short, long, value_enum, default_value = "table")]
        format: OutputFormat,
    },
    /// Inspect a volume
    Inspect {
        /// Volume name(s)
        volumes: Vec<String>,
        /// Output format
        #[arg(short, long, value_enum, default_value = "json")]
        format: OutputFormat,
    },
    /// Remove one or more volumes
    Rm {
        /// Volume name(s)
        volumes: Vec<String>,
        /// Force removal
        #[arg(short = 'f', long)]
        force: bool,
    },
    /// Remove all unused volumes
    Prune {
        /// Force prune without confirmation
        #[arg(short = 'f', long)]
        force: bool,
    },
}

#[derive(Subcommand, Debug)]
#[command(rename_all = "lower")]
enum ServiceCommands {
    /// Start a service
    Start {
        /// Service name
        name: String,
    },
    /// Stop a service
    Stop {
        /// Service name
        name: String,
        /// Force stop (SIGKILL)
        #[arg(short, long)]
        force: bool,
    },
    /// Restart a service
    Restart {
        /// Service name
        name: String,
    },
    /// List all services
    List {
        /// Output format
        #[arg(short, long, value_enum, default_value = "table")]
        format: OutputFormat,
    },
    /// Show service status
    Status {
        /// Service name
        name: String,
        /// Output format
        #[arg(short, long, value_enum, default_value = "table")]
        format: OutputFormat,
    },
    /// View service logs
    Logs {
        /// Service name
        name: String,
        /// Follow logs
        #[arg(short, long)]
        follow: bool,
        /// Number of lines
        #[arg(short, long, default_value = "100")]
        lines: usize,
    },
    /// Enable auto-start at boot
    Enable {
        /// Service name
        name: String,
    },
    /// Disable auto-start at boot
    Disable {
        /// Service name
        name: String,
    },
    /// Load service from TOML config
    Load {
        /// Service name
        name: String,
    },
    /// Create service from TOML config
    Create {
        /// Service name
        name: String,
        /// TOML config file
        #[arg(short, long)]
        config: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
#[command(rename_all = "lower")]
enum DaemonCommands {
    /// Start daemon
    Start,
    /// Stop daemon
    Stop,
    /// Show daemon status
    Status {
        /// Output format
        #[arg(short, long, value_enum, default_value = "table")]
        format: OutputFormat,
    },
    /// Reload daemon configuration
    Reload,
    /// Show daemon logs
    Logs {
        /// Follow logs
        #[arg(short, long)]
        follow: bool,
        /// Number of lines
        #[arg(short, long, default_value = "100")]
        lines: usize,
    },
}

#[derive(Subcommand, Debug)]
#[command(rename_all = "lower")]
enum WatchdogCommands {
    /// Show watchdog status
    Status {
        /// Output format
        #[arg(short, long, value_enum, default_value = "table")]
        format: OutputFormat,
    },
    /// Set watchdog policy for a service
    SetPolicy {
        /// Service name
        name: String,
        /// Restart policy
        #[arg(value_enum)]
        policy: RestartPolicyArg,
        /// Max restarts per window
        max_restarts: u32,
        /// Window in seconds
        window_secs: u64,
    },
    /// Reset watchdog state for a service
    Reset {
        /// Service name
        name: String,
    },
    /// Show watchdog events
    Events {
        /// Number of events
        #[arg(short, long, default_value = "50")]
        limit: usize,
    },
}

#[derive(Subcommand, Debug)]
#[command(rename_all = "lower")]
enum QuotaCommands {
    /// Set resource quota
    Set {
        /// Service name
        name: String,
        /// Resource type
        #[arg(value_enum)]
        resource: ResourceTypeArg,
        /// Limit value (e.g., 512M, 200, 1000)
        limit: String,
    },
    /// Get resource quotas
    Get {
        /// Service name
        name: String,
        /// Output format
        #[arg(short, long, value_enum, default_value = "table")]
        format: OutputFormat,
    },
    /// Show current resource usage
    Usage {
        /// Service name
        name: String,
        /// Output format
        #[arg(short, long, value_enum, default_value = "table")]
        format: OutputFormat,
    },
    /// List all quotas
    List {
        /// Output format
        #[arg(short, long, value_enum, default_value = "table")]
        format: OutputFormat,
    },
}

#[derive(Subcommand, Debug)]
#[command(rename_all = "lower")]
enum LogCommands {
    /// Query logs
    Query {
        /// Service name
        #[arg(short, long)]
        service: Option<String>,
        /// Minimum log level
        #[arg(long, value_enum)]
        min_level: Option<LogLevelArg>,
        /// Maximum log level
        #[arg(long, value_enum)]
        max_level: Option<LogLevelArg>,
        /// Search text
        #[arg(long)]
        grep: Option<String>,
        /// Limit results
        #[arg(short, long, default_value = "100")]
        limit: usize,
        /// Output format
        #[arg(short, long, value_enum, default_value = "table")]
        format: OutputFormat,
    },
    /// Tail logs (follow)
    Tail {
        /// Service name
        #[arg(short, long)]
        service: Option<String>,
        /// Number of lines
        #[arg(short, long, default_value = "100")]
        lines: usize,
    },
    /// Show log statistics
    Stats {
        /// Output format
        #[arg(short, long, value_enum, default_value = "table")]
        format: OutputFormat,
    },
    /// Flush log buffer
    Flush,
}

#[derive(Subcommand, Debug)]
#[command(rename_all = "lower")]
enum DbusCommands {
    /// Run the D-Bus broker in the foreground
    ///
    /// Needed because nothing else starts it: the daemon does not, and `list`
    /// only constructs a broker without listening (so it always reports none).
    Serve {
        /// Socket path (defaults to the broker's own default)
        #[arg(long)]
        socket: Option<String>,
    },
    /// List D-Bus services
    List {
        /// Output format
        #[arg(short, long, value_enum, default_value = "table")]
        format: OutputFormat,
    },
    /// Call D-Bus method
    Call {
        /// Bus name
        bus: String,
        /// Broker socket path (defaults to the broker's own default)
        #[arg(long)]
        socket: Option<String>,
        /// Object path
        path: String,
        /// Interface
        interface: String,
        /// Method
        method: String,
        /// Arguments (JSON)
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// Monitor D-Bus signals
    Monitor {
        /// Bus name
        bus: String,
    },
}

#[derive(Subcommand, Debug)]
#[command(rename_all = "lower")]
enum SocketCommands {
    /// Register socket activation
    Register {
        /// Config file
        #[arg(short, long)]
        config: PathBuf,
    },
    /// Unregister socket
    Unregister {
        /// Socket name
        name: String,
    },
    /// List socket activations
    List {
        /// Output format
        #[arg(short, long, value_enum, default_value = "table")]
        format: OutputFormat,
    },
}

#[derive(Subcommand, Debug)]
#[command(rename_all = "lower")]
enum BootCommands {
    /// Enable auto-start at boot
    Enable {
        /// Service name
        name: String,
    },
    /// Disable auto-start at boot
    Disable {
        /// Service name
        name: String,
    },
    /// Show boot configuration
    Status {
        /// Output format
        #[arg(short, long, value_enum, default_value = "table")]
        format: OutputFormat,
    },
}

#[derive(Subcommand, Debug)]
#[command(rename_all = "lower")]
enum WakelockCommands {
    /// Acquire wake lock
    Acquire {
        /// Service name
        name: String,
        /// Lock type
        #[arg(value_enum)]
        lock_type: WakeLockTypeArg,
    },
    /// Release wake lock
    Release {
        /// Service name
        name: String,
    },
    /// Show wake lock status
    Status {
        /// Output format
        #[arg(short, long, value_enum, default_value = "table")]
        format: OutputFormat,
    },
}

#[derive(Subcommand, Debug)]
#[command(rename_all = "lower")]
enum PolicyCommands {
    /// Evaluate policies for a service
    Eval {
        /// Service name
        name: String,
        /// Output format
        #[arg(short, long, value_enum, default_value = "table")]
        format: OutputFormat,
    },
    /// List policy rules
    List {
        /// Filter by tag
        #[arg(long)]
        tag: Option<String>,
        /// Output format
        #[arg(short, long, value_enum, default_value = "table")]
        format: OutputFormat,
    },
    /// Load policy from file
    Load {
        /// Policy file
        #[arg(short, long)]
        file: PathBuf,
    },
    /// Show policy presets
    Presets,
}

/// Backup/Restore management
#[derive(Subcommand, Debug)]
#[command(rename_all = "lower")]
enum BackupCommands {
    /// Create a backup
    Create {
        /// Container name or ID
        container: String,
        /// Backup type
        #[arg(short = 't', long, value_enum, default_value = "full")]
        backup_type: BackupTypeArg,
        /// Output format
        #[arg(short, long, value_enum, default_value = "table")]
        format: OutputFormat,
    },
    /// List backups
    List {
        /// Filter by container
        #[arg(short, long)]
        container: Option<String>,
        /// Output format
        #[arg(short, long, value_enum, default_value = "table")]
        format: OutputFormat,
    },
    /// Restore a backup
    Restore {
        /// Backup ID
        backup_id: String,
        /// Target container name (optional)
        #[arg(short, long)]
        target: Option<String>,
    },
    /// Delete a backup
    Delete {
        /// Backup ID
        backup_id: String,
    },
    /// Show backup status
    Status {
        /// Backup ID
        backup_id: String,
    },
    /// Show backup configuration
    Config,
}

/// Backup type
#[derive(ValueEnum, Clone, Copy, Debug)]
#[clap(rename_all = "lowercase")]
enum BackupTypeArg {
    Full,
    Incremental,
    Differential,
    ConfigOnly,
}

/// Multi-arch build service
#[derive(Subcommand, Debug)]
#[command(rename_all = "lower")]
enum BuildCommands {
    /// Build an OCI container image from a Dockerfile context
    #[command(
        long_about = "Build an OCI container image from a Dockerfile context directory.\n\n\
        EXAMPLES:\n  \
        podnext build local -t my-app:latest .\n  \
        podnext build local -f Dockerfile.custom -t my-app:v1 ./context"
    )]
    Local {
        /// Name and optionally a tag in the 'name:tag' format
        #[arg(short, long)]
        tag: Option<String>,

        /// Path to the Dockerfile (defaults to 'PATH/Dockerfile')
        #[arg(short, long)]
        file: Option<PathBuf>,

        /// Build context path (directory containing Dockerfile and assets)
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Submit a build
    Submit {
        /// Build specification file
        #[arg(short, long)]
        spec: PathBuf,
        /// Output format
        #[arg(short, long, value_enum, default_value = "table")]
        format: OutputFormat,
    },
    /// Show build status
    Status {
        /// Build ID
        build_id: String,
        /// Output format
        #[arg(short, long, value_enum, default_value = "table")]
        format: OutputFormat,
    },
    /// Cancel a build
    Cancel {
        /// Build ID
        build_id: String,
    },
    /// List builds
    List {
        /// Filter by status
        #[arg(long)]
        status: Option<String>,
        /// Output format
        #[arg(short, long, value_enum, default_value = "table")]
        format: OutputFormat,
    },
    /// Show build artifacts
    Artifacts {
        /// Build ID
        build_id: String,
    },
    /// Show build logs
    Logs {
        /// Build ID
        build_id: String,
        /// Follow logs
        #[arg(short, long)]
        follow: bool,
    },
}

/// GitOps integration
#[derive(Subcommand, Debug)]
#[command(rename_all = "lower")]
enum GitopsCommands {
    /// Sync with git repository
    Sync {
        /// Force sync
        #[arg(short, long)]
        force: bool,
    },
    /// Show GitOps status
    Status {
        /// Output format
        #[arg(short, long, value_enum, default_value = "table")]
        format: OutputFormat,
    },
    /// List managed resources
    Resources {
        /// Filter by kind
        #[arg(long)]
        kind: Option<String>,
        /// Output format
        #[arg(short, long, value_enum, default_value = "table")]
        format: OutputFormat,
    },
    /// Check for drift
    Drift {
        /// Output format
        #[arg(short, long, value_enum, default_value = "table")]
        format: OutputFormat,
    },
    /// Reconcile resources
    Reconcile {
        /// Resource name (optional, reconciles all if not specified)
        name: Option<String>,
    },
}

/// Web dashboard management
#[derive(Subcommand, Debug)]
#[command(rename_all = "lower")]
enum DashboardCommands {
    /// Start the dashboard server
    Start {
        /// Port to bind
        #[arg(short, long, default_value = "8800")]
        port: u16,
        /// Bind address
        #[arg(long, default_value = "0.0.0.0")]
        bind: String,
    },
    /// Stop the dashboard server
    Stop,
    /// Show dashboard status
    Status,
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum OutputFormat {
    Table,
    Json,
    Yaml,
    Text,
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum RestartPolicyArg {
    Never,
    OnFailure,
    Always,
    UnlessStopped,
    OnFailureOrCrash,
}

#[derive(ValueEnum, Clone, Copy, Debug, strum::Display)]
#[strum(serialize_all = "lowercase")]
enum ResourceTypeArg {
    Cpu,
    Memory,
    MemorySwap,
    IoReadBps,
    IoWriteBps,
    IoReadIops,
    IoWriteIops,
    Pids,
    Nofile,
    Network,
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum LogLevelArg {
    Emergency,
    Alert,
    Critical,
    Error,
    Warning,
    Notice,
    Info,
    Debug,
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum WakeLockTypeArg {
    Partial,
    Full,
    DozeExemption,
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum Shell {
    Bash,
    Zsh,
    Fish,
    PowerShell,
    Elvish,
}

#[tokio::main]
async fn main() {
    // Exits with the child's code when a command inside a container failed, so
    // `podnext login c -- sh -c 'exit 7'` reports 7 like every other tool. The
    // rest of the errors stay ordinary failures and exit 1.
    if let Err(err) = run().await {
        if let Some(exit) = err.downcast_ref::<proot_core::ChildExit>() {
            std::process::exit(exit.0);
        }
        eprintln!("Error: {}", err);
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    // Check for direct execution mode early (before clap parsing)
    // Direct execution: podnext [flags...] -r /rootfs [flags...] -- <cmd> ...
    // Direct execution only applies if NO subcommand is used and -r/--rootfs is BEFORE '--'
    let raw_args: Vec<String> = std::env::args().collect();
    let pn_debug = std::env::var_os("PN_NEXT_DEBUG").is_some();
    if pn_debug {
        pn_trace!("[DEBUG main] Raw args: {:?}", raw_args);
    }

    let dashdash_idx = raw_args
        .iter()
        .position(|a| a == "--")
        .unwrap_or(raw_args.len());
    let pre_dashdash_args = &raw_args[..dashdash_idx];

    const SUBCOMMANDS: &[&str] = &[
        "run",
        "login",
        "list",
        "ps",
        "kill",
        "copy",
        "sync",
        "rename",
        "reset",
        "install",
        "remove",
        "search",
        "build",
        "docker",
        "images",
        "network",
        "volume",
        "backup",
        "restore",
        "plugin",
        "help",
        "completion",
        "status",
        "info",
        "exec",
        "logs",
        "stop",
        "start",
        "restart",
        "attach",
        "commit",
    ];
    let has_subcommand = pre_dashdash_args
        .iter()
        .skip(1)
        .any(|a| SUBCOMMANDS.contains(&a.as_str()));
    let is_direct_execution = !has_subcommand
        && pre_dashdash_args
            .iter()
            .skip(1)
            .any(|a| a == "-r" || a == "--rootfs");

    if is_direct_execution {
        if pn_debug {
            pn_trace!("[DEBUG main] Direct execution mode detected");
        }

        // Parse all flags manually for direct execution
        // Format: podnext [flags...] -r <rootfs> [flags...] -- <cmd...>

        let mut rootfs: Option<String> = None;
        let mut binds = Vec::new();
        let mut pwd: Option<String> = None;
        let mut qemu: Option<String> = None;
        let mut link2symlink = false;
        let mut no_sysvipc = false;
        let mut sysvipc = false;
        let mut kernel: Option<String> = None;
        let mut hostname: Option<String> = None;
        let mut uid: Option<u32> = None;
        let mut gid: Option<u32> = None;
        let mut seccomp = false;
        let mut no_seccomp = false;
        let mut trace = false;
        let mut kill_on_exit = false;
        let mut no_kill_on_exit = false;
        let mut env = Vec::new();
        let mut verbose = false;
        let mut cmd_args: Vec<String> = Vec::new();

        let mut i = 1; // Skip program name

        // Parse all flags until --
        while i < raw_args.len() {
            let arg = &raw_args[i];
            if arg == "--" {
                // Remaining args are the command
                cmd_args = raw_args[i + 1..].to_vec();
                break;
            }

            match arg.as_str() {
                "-r" | "--rootfs" => {
                    i += 1;
                    if i < raw_args.len() {
                        rootfs = Some(raw_args[i].clone());
                    }
                }
                "-b" | "--bind" => {
                    i += 1;
                    if i < raw_args.len() {
                        binds.push(raw_args[i].clone());
                    }
                }
                "-w" | "--pwd" => {
                    i += 1;
                    if i < raw_args.len() {
                        pwd = Some(raw_args[i].clone());
                    }
                }
                "-q" | "--qemu" => {
                    i += 1;
                    if i < raw_args.len() {
                        qemu = Some(raw_args[i].clone());
                    }
                }
                "-0" | "--link2symlink" => {
                    link2symlink = true;
                }
                "--no-sysvipc" => {
                    no_sysvipc = true;
                }
                "--sysvipc" => {
                    sysvipc = true;
                }
                "--kernel" => {
                    i += 1;
                    if i < raw_args.len() {
                        kernel = Some(raw_args[i].clone());
                    }
                }
                "--hostname" => {
                    i += 1;
                    if i < raw_args.len() {
                        hostname = Some(raw_args[i].clone());
                    }
                }
                "--uid" => {
                    i += 1;
                    if i < raw_args.len() {
                        uid = raw_args[i].parse().ok();
                    }
                }
                "--gid" => {
                    i += 1;
                    if i < raw_args.len() {
                        gid = raw_args[i].parse().ok();
                    }
                }
                "--seccomp" => {
                    seccomp = true;
                }
                "--no-seccomp" => {
                    no_seccomp = true;
                }
                "--trace" => {
                    trace = true;
                }
                "--kill-on-exit" => {
                    kill_on_exit = true;
                }
                "--no-kill-on-exit" => {
                    no_kill_on_exit = true;
                }
                "-e" | "--env" => {
                    i += 1;
                    if i < raw_args.len() {
                        env.push(raw_args[i].clone());
                    }
                }
                "-v" | "--verbose" => {
                    verbose = true;
                }
                _ => {
                    // Unknown flag, skip
                }
            }
            i += 1;
        }

        if rootfs.is_none() {
            eprintln!("Error: -r/--rootfs is required for direct execution mode");
            std::process::exit(1);
        }

        if cmd_args.is_empty() {
            eprintln!("Error: Direct execution mode requires a command after --");
            std::process::exit(1);
        }

        if pn_debug {
            pn_trace!("[DEBUG main] Calling execute_direct_manual with all flags");
        }
        // For now, just call execute_direct_manual with basic args
        // TODO: Pass all parsed flags to a new execute_direct_manual_full function
        return execute_direct_manual(rootfs.unwrap(), &cmd_args, verbose).await;
    }

    // Parse trailing arguments for direct execution mode (-- <cmd> ...) for clap-parsed case
    let args: Vec<String> = std::env::args().collect();
    let cli_trailing_args: Vec<String> = if let Some(pos) = args.iter().position(|a| a == "--") {
        args[pos + 1..].to_vec()
    } else {
        Vec::new()
    };

    let cli = Cli::parse();

    // Direct execution mode (like proot) - clap parsed case
    if cli.command.is_none() && cli.rootfs.is_some() {
        return execute_direct(&cli, &cli_trailing_args).await;
    }

    // Subcommand mode
    if let Some(cmd) = cli.command {
        pn_trace!("DEBUG main: Executing command: {:?}", cmd);
        return execute_command(cmd).await;
    }

    print_help();
    Ok(())
}

async fn execute_direct(cli: &Cli, trailing_args: &[String]) -> Result<()> {
    if trailing_args.is_empty() {
        bail!("Direct execution mode requires a command. Use 'podnext -r /path/to/rootfs -- <command>'");
    }

    pn_trace!("[DEBUG direct] Rootfs: {:?}", cli.rootfs);
    pn_trace!("[DEBUG direct] Binds: {:?}", cli.bind);
    pn_trace!("[DEBUG direct] Working dir: {:?}", cli.pwd);
    pn_trace!("[DEBUG direct] QEMU: {:?}", cli.qemu);
    pn_trace!("[DEBUG direct] Link2symlink: {}", cli.link2symlink);
    pn_trace!("[DEBUG direct] No sysvipc: {}", cli.no_sysvipc);
    pn_trace!("[DEBUG direct] Sysvipc: {}", cli.sysvipc);
    pn_trace!("[DEBUG direct] Kernel: {:?}", cli.kernel);
    pn_trace!("[DEBUG direct] Hostname: {:?}", cli.hostname);
    pn_trace!("[DEBUG direct] UID: {:?}", cli.uid);
    pn_trace!("[DEBUG direct] GID: {:?}", cli.gid);
    pn_trace!("[DEBUG direct] Seccomp: {}", cli.seccomp);
    pn_trace!("[DEBUG direct] No seccomp: {}", cli.no_seccomp);
    pn_trace!("[DEBUG direct] Trace: {}", cli.trace);
    pn_trace!("[DEBUG direct] Kill on exit: {}", cli.kill_on_exit);
    pn_trace!("[DEBUG direct] No kill on exit: {}", cli.no_kill_on_exit);
    pn_trace!("[DEBUG direct] Env: {:?}", cli.env);
    pn_trace!("[DEBUG direct] Command: {:?}", trailing_args);

    // Create a temporary container config for direct execution
    let rootfs_path = cli.rootfs.as_ref().unwrap();

    // Use proot-core directly for direct execution
    let mut config = proot_core::ProotConfig::default();
    config.verbose = cli.verbose;
    let mut engine = proot_core::ProotEngine::new(config)?;

    engine.config.rootfs = Some(rootfs_path.into());

    // Process bind mounts
    for bind in &cli.bind {
        engine.config.bind.push(bind.clone());
    }

    // Set working directory
    if let Some(pwd) = &cli.pwd {
        engine.config.cwd = Some(pwd.into());
    }

    // Set environment variables
    for env in &cli.env {
        engine.config.env.push(env.clone());
    }

    // Set kernel release
    if let Some(kernel) = &cli.kernel {
        engine.config.kernel_release = Some(kernel.clone());
    }

    // Set hostname
    if let Some(hostname) = &cli.hostname {
        engine.config.hostname = Some(hostname.clone());
    }

    // Set UID/GID
    if let Some(uid) = cli.uid {
        engine.config.uid = Some(uid);
    }
    if let Some(gid) = cli.gid {
        engine.config.gid = Some(gid);
    }

    // Set seccomp
    if cli.seccomp {
        engine.config.seccomp = true;
    }
    if cli.no_seccomp {
        engine.config.seccomp = false;
    }

    // Set trace
    if cli.trace {
        engine.config.trace = true;
    }

    // Set kill-on-exit
    if cli.kill_on_exit {
        engine.config.kill_on_exit = true;
    }
    if cli.no_kill_on_exit {
        engine.config.kill_on_exit = false;
    }

    // Set sysvipc
    if cli.no_sysvipc {
        engine.config.sysvipc = false;
    }
    if cli.sysvipc {
        engine.config.sysvipc = true;
    }

    // Set link2symlink
    if cli.link2symlink {
        engine.config.link2symlink = true;
    }

    // Set command
    engine.config.command = trailing_args.to_vec();

    // Execute and wait for completion
    let exit_code = engine.execute().await? as i32;
    if exit_code != 0 {
        return Err(anyhow::Error::new(proot_core::ChildExit(exit_code)));
    }

    Ok(())
}

async fn execute_direct_manual(rootfs: String, cmd_args: &[String], verbose: bool) -> Result<()> {
    if cmd_args.is_empty() {
        bail!("Direct execution mode requires a command. Use 'podnext -r /path/to/rootfs -- <command>'");
    }

    pn_trace!("[DEBUG direct] Rootfs: {:?}", rootfs);
    pn_trace!("[DEBUG direct] Command: {:?}", cmd_args);
    pn_trace!("[DEBUG direct] Verbose: {}", verbose);

    // Use proot-core directly for direct execution
    let mut config = proot_core::ProotConfig::default();
    config.verbose = if verbose { 1 } else { 0 };
    let mut engine = proot_core::ProotEngine::new(config)?;

    engine.config.rootfs = Some(rootfs.into());
    engine.config.command = cmd_args.to_vec();

    // Execute and wait for completion
    let exit_code = engine.execute().await? as i32;
    if exit_code != 0 {
        return Err(anyhow::Error::new(proot_core::ChildExit(exit_code)));
    }

    Ok(())
}

async fn execute_command(cmd: Commands) -> Result<()> {
    pn_trace!("DEBUG execute_command: START {:?}", cmd);
    match cmd {
        // Container commands
        Commands::Install { image, name, force } => cmd_install(image, name, force).await,
        Commands::List {
            all,
            no_all,
            format,
        } => cmd_list(all, no_all, format).await,
        Commands::Login {
            container,
            user,
            isolated,
            command,
        } => cmd_login(container, user, isolated, command).await,
        Commands::Run {
            container,
            command,
            rm,
            detach,
            name,
            network,
            volume,
            publish,
            entrypoint,
            tty,
            interactive,
            user,
            workdir,
            isolated,
        } => {
            cmd_run(
                container,
                command,
                rm,
                detach,
                name,
                network,
                volume,
                publish,
                entrypoint,
                tty,
                interactive,
                user,
                workdir,
                isolated,
            )
            .await
        }
        Commands::Ps { format } => cmd_ps(format).await,
        Commands::Sessions { format } => cmd_sessions(format).await,
        Commands::Attach {
            container,
            user,
            command,
        } => cmd_attach(container, user, command).await,
        Commands::Kill { container } => cmd_kill(container).await,
        Commands::Remove { name, force } => cmd_remove(name, force).await,
        Commands::Rename { oldname, newname } => cmd_rename(oldname, newname).await,
        Commands::Copy {
            source,
            destination,
            recursive,
            move_,
        } => cmd_copy(source, destination, recursive, move_).await,
        Commands::Reset { name } => cmd_reset(name).await,
        Commands::Sync {
            source,
            destination,
            delete,
        } => cmd_sync(source, destination, delete).await,
        Commands::Search {
            query,
            limit,
            format,
        } => cmd_search(query, limit, format).await,
        Commands::Image(cmd) => execute_image_command(cmd).await,
        Commands::Network(cmd) => execute_network_command(cmd).await,
        Commands::Volume(cmd) => execute_volume_command(cmd).await,
        Commands::Info { format } => cmd_info(format).await,
        Commands::Version { format } => cmd_version(format).await,

        // Service orchestration commands
        Commands::Service(cmd) => execute_service_command(cmd).await,
        Commands::Daemon(cmd) => execute_daemon_command(cmd).await,
        Commands::Watchdog(cmd) => execute_watchdog_command(cmd).await,
        Commands::Quota(cmd) => execute_quota_command(cmd).await,
        Commands::Log(cmd) => execute_log_command(cmd).await,
        Commands::Dbus(cmd) => execute_dbus_command(cmd).await,
        Commands::Socket(cmd) => execute_socket_command(cmd).await,
        Commands::Boot(cmd) => execute_boot_command(cmd).await,
        Commands::Wakelock(cmd) => execute_wakelock_command(cmd).await,
        Commands::Policy(cmd) => execute_policy_command(cmd).await,

        // Phase 7: Platform Maturity commands
        Commands::Backup(cmd) => execute_backup_command(cmd).await,
        Commands::Build(cmd) => execute_build_command(cmd).await,
        Commands::Gitops(cmd) => execute_gitops_command(cmd).await,
        Commands::Dashboard(cmd) => execute_dashboard_command(cmd).await,
        Commands::Docker { container, args } => cmd_docker(container, args).await,

        // Utilities
        Commands::Completion { shell } => cmd_completion(shell).await,
    }
}

// Output formatting helpers
fn output_table<T: Tabled>(items: &[T]) {
    println!("{}", Table::new(items).to_string());
}

fn output_json<T: serde::Serialize>(items: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(items)?);
    Ok(())
}

fn output_yaml<T: serde::Serialize>(items: &T) -> Result<()> {
    println!("{}", serde_yaml::to_string(items)?);
    Ok(())
}

// Container command implementations (existing)
async fn cmd_install(image: String, name: Option<String>, force: bool) -> Result<()> {
    let container_name = match &name {
        Some(explicit) => explicit.clone(),
        None => default_container_name(&image),
    };

    // An explicit `--name ""` used to be accepted: the container directory then
    // collapsed onto the containers root itself, creating an orphan that `list`
    // showed with a blank name and `remove` could never match. Derived names are
    // validated too, so no install can create an unusable path.
    proot_distro_core::NameValidator::new().validate_container_name(&container_name)?;

    let _rootfs: PathBuf = install_image_native(&image, &container_name, force, None).await?;
    Ok(())
}

/// Derive a container name from an image reference.
///
/// `debian:13` and `ghcr.io:5000/acme/web:1.2` both yield a single path component
/// (`debian`, `web`): a registry host, port, or namespace must never become a
/// directory component under `~/.podnext/containers/`.
fn default_container_name(image: &str) -> String {
    let last = image.rsplit('/').next().unwrap_or(image);
    last.split(':').next().unwrap_or(last).to_string()
}

async fn cmd_list(all: bool, no_all: bool, format: OutputFormat) -> Result<()> {
    let show_all = all && !no_all;
    let containers = list_containers_native(show_all).await?;

    #[derive(Tabled, Serialize)]
    struct ContainerRow {
        #[tabled(rename = "NAME")]
        name: String,
        #[tabled(rename = "DISTRO")]
        distro: String,
        #[tabled(rename = "VERSION")]
        version: String,
        #[tabled(rename = "ARCH")]
        arch: String,
        #[tabled(rename = "STATUS")]
        status: String,
        #[tabled(rename = "CREATED")]
        created_at: String,
    }

    let rows: Vec<ContainerRow> = containers
        .iter()
        .map(|c| ContainerRow {
            name: c.name.clone(),
            distro: c.distro.clone(),
            version: c.version.clone(),
            arch: c.arch.clone(),
            status: format!("{:?}", c.status),
            created_at: c.created_at.format("%Y-%m-%d %H:%M").to_string(),
        })
        .collect();

    match format {
        OutputFormat::Table => output_table(&rows),
        OutputFormat::Json => output_json(&containers)?,
        OutputFormat::Yaml => output_yaml(&containers)?,
        OutputFormat::Text => {
            for c in containers {
                println!(
                    "{} ({}:{}) [{}]",
                    c.name,
                    c.distro,
                    c.version,
                    format!("{:?}", c.status)
                );
            }
        }
    }
    Ok(())
}

async fn cmd_login(
    container: String,
    user: Option<String>,
    isolated: bool,
    command: Vec<String>,
) -> Result<()> {
    let cmd = if command.is_empty() {
        vec!["/bin/sh".to_string()]
    } else {
        command
    };

    login_container_native(&container, user, isolated, cmd).await
}

async fn cmd_run(
    container: String,
    command: Vec<String>,
    _rm: bool,
    detach: bool,
    _name: Option<String>,
    _network: Option<String>,
    _volume: Vec<String>,
    _publish: Vec<String>,
    _entrypoint: Option<String>,
    _tty: bool,
    _interactive: bool,
    _user: Option<String>,
    _workdir: Option<String>,
    isolated: bool,
) -> Result<()> {
    if detach {
        eprintln!("Warning: --detach/-d not fully implemented yet, running in foreground");
    }

    run_container_native(&container, command, isolated).await
}

async fn cmd_ps(format: OutputFormat) -> Result<()> {
    let sessions = list_sessions_native_full().await?;
    let active_disk_sessions = proot_distro_core::session::active_sessions().unwrap_or_default();

    #[derive(Tabled, Serialize)]
    struct SessionRow {
        #[tabled(rename = "ID")]
        id: String,
        #[tabled(rename = "CONTAINER")]
        container: String,
        #[tabled(rename = "USER")]
        user: String,
        #[tabled(rename = "COMMAND")]
        command: String,
        #[tabled(rename = "CWD")]
        cwd: String,
        #[tabled(rename = "PID")]
        pid: String,
        #[tabled(rename = "STATUS")]
        status: String,
        #[tabled(rename = "CREATED")]
        created: String,
        #[tabled(rename = "STARTED")]
        started: String,
    }

    let mut rows: Vec<SessionRow> = sessions
        .iter()
        .map(|s| SessionRow {
            id: s.id.clone(),
            container: s.container_name.clone(),
            user: s.user.clone(),
            command: s.command.join(" "),
            cwd: s.cwd.clone(),
            pid: s.pid.map_or("-".to_string(), |p| p.to_string()),
            status: format!("{:?}", s.status),
            created: s.created_at.format("%Y-%m-%d %H:%M").to_string(),
            started: s.started_at.map_or("-".to_string(), |dt| {
                dt.format("%Y-%m-%d %H:%M").to_string()
            }),
        })
        .collect();

    for act in active_disk_sessions {
        if !rows.iter().any(|r| r.pid == act.pid.to_string()) {
            rows.push(SessionRow {
                id: format!("sess-{}", act.pid),
                container: act.container_name,
                user: act.login_uid,
                command: act.cmdline.join(" "),
                cwd: "-".to_string(),
                pid: act.pid.to_string(),
                status: "Running".to_string(),
                created: "-".to_string(),
                started: "-".to_string(),
            });
        }
    }

    match format {
        OutputFormat::Table => output_table(&rows),
        OutputFormat::Json => output_json(&sessions)?,
        OutputFormat::Yaml => output_yaml(&sessions)?,
        OutputFormat::Text => {
            for s in sessions {
                println!(
                    "{} {} {} {} {} {}",
                    s.id,
                    s.container_name,
                    s.user,
                    s.pid.map_or("-".to_string(), |p| p.to_string()),
                    format!("{:?}", s.status),
                    s.command.join(" ")
                );
            }
        }
    }
    Ok(())
}

async fn cmd_sessions(format: OutputFormat) -> Result<()> {
    let sessions = list_sessions_native().await?;

    #[derive(Tabled, Serialize)]
    struct SessionRow {
        #[tabled(rename = "CONTAINER")]
        container: String,
        #[tabled(rename = "USER")]
        user: String,
        #[tabled(rename = "PID")]
        pid: String,
        #[tabled(rename = "STATUS")]
        status: String,
        #[tabled(rename = "STARTED")]
        started: String,
    }

    let rows: Vec<SessionRow> = sessions
        .iter()
        .map(|s| SessionRow {
            container: s.container_name.clone(),
            user: s.user.clone(),
            pid: s.pid.map_or("-".to_string(), |p| p.to_string()),
            status: s.status.clone(),
            started: s.started_at.format("%Y-%m-%d %H:%M").to_string(),
        })
        .collect();

    match format {
        OutputFormat::Table => output_table(&rows),
        OutputFormat::Json => output_json(&sessions)?,
        OutputFormat::Yaml => output_yaml(&sessions)?,
        OutputFormat::Text => {
            for s in sessions {
                println!(
                    "{} {} {} {}",
                    s.container_name,
                    s.user,
                    s.pid.map_or("-".to_string(), |p| p.to_string()),
                    s.status
                );
            }
        }
    }
    println!("\nNote: Use 'podnext attach <container>' to create a new session.");
    Ok(())
}

async fn cmd_attach(container: String, user: Option<String>, command: Vec<String>) -> Result<()> {
    let cmd = if command.is_empty() {
        vec!["/bin/bash".to_string()]
    } else {
        command
    };

    login_container_native(&container, user, false, cmd).await
}

async fn cmd_docker(container_name: Option<String>, args: Vec<String>) -> Result<()> {
    let container_mgr = proot_distro_core::ContainerManager::new()?;
    let backend: Box<dyn proot_distro_core::RuntimeBackend> = if let Some(ref name) = container_name
    {
        let c = container_mgr.get(name).await?;
        container_mgr.get_backend(&c)
    } else {
        Box::new(proot_distro_core::UmlBackend::new())
    };

    if !backend.supports_kernel_features() {
        anyhow::bail!(
            "The configured runtime backend '{}' does not support kernel features required by Docker.\n\
             Reason: Docker requires namespaces (mount, pid, net) and cgroups v2, which PRoot cannot provide.\n\
             Resolution: Start the UML backend with 'uml4termux up' and use '--runtime uml'.",
            format!("{:?}", backend.kind()).to_lowercase()
        );
    }

    let status = backend.status().await?;
    if !status.running {
        anyhow::bail!(
            "UML kernel runtime is not running (status: {}). Please start it with 'uml4termux up'.",
            status.details
        );
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

async fn cmd_kill(container: String) -> Result<()> {
    if let Ok(active) = proot_distro_core::session::active_sessions() {
        let mut killed_any = false;
        for s in active {
            if s.container_name == container || container == "all" || container == "--all" {
                if let Ok(()) = proot_distro_core::session::kill_session(s.pid, libc::SIGQUIT) {
                    println!(
                        "Sent SIGQUIT to session PID {} ({})",
                        s.pid, s.container_name
                    );
                    killed_any = true;
                }
            }
        }
        if killed_any {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }
    }
    kill_container_native(&container).await
}

async fn cmd_remove(name: String, force: bool) -> Result<()> {
    remove_container_native(name, force).await
}

async fn cmd_info(format: OutputFormat) -> Result<()> {
    let info = SystemInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        platform: "Termux/Android".to_string(),
        rust_version: "1.80+".to_string(),
        backend: "proot-distro-core".to_string(),
    };

    #[derive(Tabled, Serialize)]
    struct InfoRow {
        #[tabled(rename = "VERSION")]
        version: String,
        #[tabled(rename = "PLATFORM")]
        platform: String,
        #[tabled(rename = "RUST VERSION")]
        rust_version: String,
        #[tabled(rename = "BACKEND")]
        backend: String,
    }

    let row = InfoRow {
        version: info.version.clone(),
        platform: info.platform.clone(),
        rust_version: info.rust_version.clone(),
        backend: info.backend.clone(),
    };

    let serializable = SerializableInfo {
        version: info.version.clone(),
        platform: info.platform.clone(),
        rust_version: info.rust_version.clone(),
        backend: info.backend.clone(),
    };

    match format {
        OutputFormat::Table => output_table(&[row]),
        OutputFormat::Json => output_json(&serializable)?,
        OutputFormat::Yaml => output_yaml(&serializable)?,
        OutputFormat::Text => println!(
            "podnext {} - PRoot Distro Next for Termux/Android",
            env!("CARGO_PKG_VERSION")
        ),
    }
    Ok(())
}

async fn cmd_search(query: String, limit: usize, format: OutputFormat) -> Result<()> {
    if query.trim().is_empty() {
        #[derive(Tabled, Serialize)]
        struct CuratedDistro {
            #[tabled(rename = "DISTRIBUTION")]
            name: &'static str,
            #[tabled(rename = "RECOMMENDED TAG")]
            tag: &'static str,
            #[tabled(rename = "DESCRIPTION")]
            description: &'static str,
            #[tabled(rename = "INSTALL COMMAND")]
            install_cmd: &'static str,
        }

        let curated = vec![
            CuratedDistro {
                name: "debian",
                tag: "13 (trixie) / 12 (bookworm)",
                description: "Stable, lightweight, vast apt package ecosystem",
                install_cmd: "podnext install debian:13",
            },
            CuratedDistro {
                name: "ubuntu",
                tag: "24.04 (noble) / 22.04 (jammy)",
                description: "Standard LTS Linux development environment",
                install_cmd: "podnext install ubuntu:22.04",
            },
            CuratedDistro {
                name: "alpine",
                tag: "latest / 3.20",
                description: "Ultra-small ~5MB musl/busybox base",
                install_cmd: "podnext install alpine:latest",
            },
            CuratedDistro {
                name: "archlinux",
                tag: "latest",
                description: "Rolling-release Arch Linux with pacman",
                install_cmd: "podnext install archlinux:latest",
            },
            CuratedDistro {
                name: "fedora",
                tag: "40",
                description: "Modern RPM-based Fedora workstation",
                install_cmd: "podnext install fedora:40",
            },
            CuratedDistro {
                name: "rockylinux",
                tag: "9",
                description: "Enterprise RHEL binary-compatible distro",
                install_cmd: "podnext install rockylinux:9",
            },
            CuratedDistro {
                name: "almalinux",
                tag: "9",
                description: "Enterprise Linux 1:1 RHEL compatible",
                install_cmd: "podnext install almalinux:9",
            },
            CuratedDistro {
                name: "voidlinux",
                tag: "latest",
                description: "Independent distro with xbps package manager",
                install_cmd: "podnext install voidlinux/voidlinux:latest",
            },
            CuratedDistro {
                name: "kali-rolling",
                tag: "latest",
                description: "Penetration testing & forensics tools platform",
                install_cmd: "podnext install kalilinux/kali-rolling:latest",
            },
        ];

        match format {
            OutputFormat::Table => {
                println!("Curated Official Distributions for PDN:\n");
                output_table(&curated);
                println!("\nHint: Run 'podnext install <distro:tag>' or search Docker Hub with 'podnext search <term>'.");
            }
            OutputFormat::Json => output_json(&curated)?,
            OutputFormat::Yaml => output_yaml(&curated)?,
            OutputFormat::Text => {
                for d in curated {
                    println!("{} ({}) - {}", d.name, d.tag, d.install_cmd);
                }
            }
        }
        return Ok(());
    }

    let images = search_docker_hub(&query, limit).await?;

    #[derive(Tabled, Serialize)]
    struct ImageRow {
        #[tabled(rename = "NAME")]
        name: String,
        #[tabled(rename = "DESCRIPTION")]
        description: String,
        #[tabled(rename = "STARS")]
        stars: u32,
        #[tabled(rename = "PULLS")]
        pulls: u64,
        #[tabled(rename = "OFFICIAL")]
        official: String,
    }

    let rows: Vec<ImageRow> = images
        .iter()
        .map(|img| ImageRow {
            name: img.repo_name.clone(),
            description: if img.short_description.len() > 48 {
                format!("{}...", &img.short_description[..48])
            } else {
                img.short_description.clone()
            },
            stars: img.star_count,
            pulls: img.pull_count,
            official: if img.is_official {
                "[OK]".to_string()
            } else {
                "".to_string()
            },
        })
        .collect();

    let serializable: Vec<SerializableImage> = images
        .iter()
        .map(|img| SerializableImage {
            repo_name: img.repo_name.clone(),
            short_description: img.short_description.clone(),
            star_count: img.star_count,
            pull_count: img.pull_count,
            is_official: img.is_official,
            is_automated: img.is_automated,
        })
        .collect();

    match format {
        OutputFormat::Table => {
            output_table(&rows);
            println!("\nHint: To install an image, run: podnext install <name>[:tag]");
        }
        OutputFormat::Json => output_json(&serializable)?,
        OutputFormat::Yaml => output_yaml(&serializable)?,
        OutputFormat::Text => {
            for img in images {
                println!(
                    "{} - {} (stars: {})",
                    img.repo_name, img.short_description, img.star_count
                );
            }
        }
    }
    Ok(())
}

async fn execute_image_command(cmd: ImageCommands) -> Result<()> {
    match cmd {
        ImageCommands::Pull {
            image,
            platform,
            all_tags,
        } => cmd_image_pull(image, platform, all_tags).await,
        ImageCommands::Push { image } => cmd_image_push(image).await,
        ImageCommands::Rm { images, force } => cmd_image_rm(images, force).await,
        ImageCommands::Tag { source, target } => cmd_image_tag(source, target).await,
        ImageCommands::History {
            image,
            no_trunc,
            format,
        } => cmd_image_history(image, no_trunc, format).await,
        ImageCommands::Inspect { images, format } => cmd_image_inspect(images, format).await,
        ImageCommands::Ls { format, all } => cmd_image_ls(format, all).await,
    }
}

async fn cmd_image_pull(image: String, platform: Option<String>, all_tags: bool) -> Result<()> {
    eprintln!("Pulling image: {}", image);
    if let Some(p) = platform {
        eprintln!("Platform: {}", p);
    }
    if all_tags {
        eprintln!("Pulling all tags");
    }

    // TODO: Implement actual image pull using proot-distro-core or direct OCI registry calls
    // For now, delegate to install command logic
    let container_mgr = proot_distro_core::ContainerManager::new()?;
    let install_options = proot_distro_core::InstallOptions {
        source: proot_distro_core::ImageSource::Docker(image.clone()),
        name: image.split(':').next().unwrap_or(&image).to_string(),
        config: Default::default(),
        distro_type: proot_distro_core::DistroType::default(),
    };
    container_mgr
        .create(&install_options.name, &install_options.config)
        .await?;
    eprintln!("Image {} pulled successfully", image);
    Ok(())
}

async fn cmd_image_push(image: String) -> Result<()> {
    eprintln!("Pushing image: {}", image);
    let mut image_mgr = proot_distro_core::ImageManager::new()?;
    image_mgr.push(&image, None, None).await?;
    Ok(())
}

async fn cmd_image_rm(images: Vec<String>, force: bool) -> Result<()> {
    let container_mgr = proot_distro_core::ContainerManager::new()?;

    for image in images {
        if !force {
            print!("Remove image '{}'? [y/N]: ", image);
            io::stdout().flush()?;
            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            if !input.trim().eq_ignore_ascii_case("y") {
                println!("Cancelled.");
                continue;
            }
        }

        // For now, just remove the container if it exists
        match container_mgr.remove(&image).await {
            Ok(_) => println!("Removed image: {}", image),
            Err(e) => eprintln!("Failed to remove image {}: {}", image, e),
        }
    }
    Ok(())
}

async fn cmd_image_tag(source: String, target: String) -> Result<()> {
    eprintln!("Tagging {} -> {}", source, target);
    let mut image_mgr = proot_distro_core::ImageManager::new()?;
    image_mgr.tag(&source, &target)?;
    eprintln!("Tagged {} as {}", source, target);
    Ok(())
}

async fn cmd_image_history(image: String, no_trunc: bool, format: OutputFormat) -> Result<()> {
    eprintln!("Showing history for image: {}", image);
    let image_mgr = proot_distro_core::ImageManager::new()?;
    let history = image_mgr.history(&image)?;

    #[derive(Tabled, Serialize)]
    struct HistoryRow {
        #[tabled(rename = "IMAGE")]
        image: String,
        #[tabled(rename = "CREATED")]
        created: String,
        #[tabled(rename = "CREATED BY")]
        created_by: String,
        #[tabled(rename = "SIZE")]
        size: String,
        #[tabled(rename = "COMMENT")]
        comment: String,
    }

    let rows: Vec<HistoryRow> = history
        .iter()
        .map(|h| HistoryRow {
            image: if no_trunc {
                h.id.clone()
            } else {
                format!("{}...", &h.id[..12.min(h.id.len())])
            },
            created: h.created.format("%Y-%m-%d %H:%M:%S").to_string(),
            created_by: h.created_by.clone(),
            size: format_size(h.size),
            comment: if no_trunc {
                h.comment.clone()
            } else {
                if h.comment.len() > 50 {
                    format!("{}...", &h.comment[..50])
                } else {
                    h.comment.clone()
                }
            },
        })
        .collect();

    match format {
        OutputFormat::Table => output_table(&rows),
        OutputFormat::Json => output_json(&history)?,
        OutputFormat::Yaml => output_yaml(&history)?,
        OutputFormat::Text => {
            for h in history {
                println!(
                    "{} {} {} {}",
                    if no_trunc {
                        h.id.clone()
                    } else {
                        format!("{}...", &h.id[..12.min(h.id.len())])
                    },
                    h.created.format("%Y-%m-%d %H:%M:%S"),
                    h.created_by,
                    format_size(h.size)
                );
            }
        }
    }
    Ok(())
}

async fn cmd_image_inspect(images: Vec<String>, format: OutputFormat) -> Result<()> {
    eprintln!("Inspecting images: {:?}", images);
    let image_mgr = proot_distro_core::ImageManager::new()?;

    let mut results = Vec::new();
    for image_ref in images {
        match image_mgr.inspect(&image_ref) {
            Ok(info) => results.push(info),
            Err(e) => eprintln!("Failed to inspect {}: {}", image_ref, e),
        }
    }

    match format {
        OutputFormat::Table => {
            // For table, show summary
            #[derive(Tabled, Serialize)]
            struct InspectRow {
                #[tabled(rename = "IMAGE")]
                image: String,
                #[tabled(rename = "ID")]
                id: String,
                #[tabled(rename = "CREATED")]
                created: String,
                #[tabled(rename = "SIZE")]
                size: String,
                #[tabled(rename = "ARCH")]
                arch: String,
                #[tabled(rename = "OS")]
                os: String,
            }

            let rows: Vec<InspectRow> = results
                .iter()
                .map(|i| InspectRow {
                    image: i.repo_tags.first().cloned().unwrap_or_default(),
                    id: format!("{}...", &i.id[..12.min(i.id.len())]),
                    created: i.created.format("%Y-%m-%d %H:%M:%S").to_string(),
                    size: format_size(i.size),
                    arch: i.architecture.clone(),
                    os: i.os.clone(),
                })
                .collect();
            output_table(&rows);
        }
        OutputFormat::Json => output_json(&results)?,
        OutputFormat::Yaml => output_yaml(&results)?,
        OutputFormat::Text => {
            for i in results {
                println!(
                    "Image: {}",
                    i.repo_tags.first().cloned().unwrap_or_default()
                );
                println!("  ID: {}", i.id);
                println!("  Created: {}", i.created.format("%Y-%m-%d %H:%M:%S"));
                println!("  Size: {}", format_size(i.size));
                println!("  Architecture: {}", i.architecture);
                println!("  OS: {}", i.os);
                println!();
            }
        }
    }
    Ok(())
}

async fn cmd_image_ls(format: OutputFormat, _all: bool) -> Result<()> {
    let container_mgr = proot_distro_core::ContainerManager::new()?;
    let containers = container_mgr.list().await?;

    #[derive(Tabled, Serialize)]
    struct ImageRow {
        #[tabled(rename = "REPOSITORY")]
        repository: String,
        #[tabled(rename = "TAG")]
        tag: String,
        #[tabled(rename = "IMAGE ID")]
        image_id: String,
        #[tabled(rename = "CREATED")]
        created: String,
        #[tabled(rename = "SIZE")]
        size: String,
    }

    let rows: Vec<ImageRow> = containers
        .iter()
        .map(|c| ImageRow {
            repository: c.distro.clone(),
            tag: c.version.clone(),
            image_id: c.name.clone(),
            created: c.created_at.format("%Y-%m-%d %H:%M").to_string(),
            size: format_size(c.rootfs_path.metadata().map(|m| m.len()).unwrap_or(0)),
        })
        .collect();

    match format {
        OutputFormat::Table => output_table(&rows),
        OutputFormat::Json => output_json(&containers)?,
        OutputFormat::Yaml => output_yaml(&containers)?,
        OutputFormat::Text => {
            for c in containers {
                println!(
                    "{} {} {} {}",
                    c.distro,
                    c.version,
                    c.name,
                    c.created_at.format("%Y-%m-%d %H:%M")
                );
            }
        }
    }
    Ok(())
}

async fn cmd_version(format: OutputFormat) -> Result<()> {
    let version_info = VersionInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        git_commit: option_env!("GIT_COMMIT").unwrap_or("unknown").to_string(),
        build_date: option_env!("BUILD_DATE").unwrap_or("unknown").to_string(),
        rust_version: "1.80+".to_string(),
    };

    match format {
        OutputFormat::Table => output_table(&[version_info]),
        OutputFormat::Json => output_json(&version_info)?,
        OutputFormat::Yaml => output_yaml(&version_info)?,
        OutputFormat::Text => println!("podnext {}", env!("CARGO_PKG_VERSION")),
    }
    Ok(())
}

async fn cmd_completion(shell: Shell) -> Result<()> {
    let mut cmd = Cli::command();
    let name = "podnext".to_string();

    match shell {
        Shell::Bash => clap_complete::generate(
            clap_complete::Shell::Bash,
            &mut cmd,
            name,
            &mut std::io::stdout(),
        ),
        Shell::Zsh => clap_complete::generate(
            clap_complete::Shell::Zsh,
            &mut cmd,
            name,
            &mut std::io::stdout(),
        ),
        Shell::Fish => clap_complete::generate(
            clap_complete::Shell::Fish,
            &mut cmd,
            name,
            &mut std::io::stdout(),
        ),
        Shell::PowerShell => clap_complete::generate(
            clap_complete::Shell::PowerShell,
            &mut cmd,
            name,
            &mut std::io::stdout(),
        ),
        Shell::Elvish => clap_complete::generate(
            clap_complete::Shell::Elvish,
            &mut cmd,
            name,
            &mut std::io::stdout(),
        ),
    }
    Ok(())
}

fn print_help() {
    let mut cmd = Cli::command();
    cmd.print_help().unwrap();
    println!();
}

// Output format helpers
#[derive(Tabled)]
struct SystemInfo {
    #[tabled(rename = "Version")]
    version: String,
    #[tabled(rename = "Platform")]
    platform: String,
    #[tabled(rename = "Rust Version")]
    rust_version: String,
    #[tabled(rename = "Backend")]
    backend: String,
}

#[derive(Tabled, serde::Serialize)]
struct VersionInfo {
    #[tabled(rename = "Version")]
    version: String,
    #[tabled(rename = "Git Commit")]
    git_commit: String,
    #[tabled(rename = "Build Date")]
    build_date: String,
    #[tabled(rename = "Rust Version")]
    rust_version: String,
}

async fn execute_network_command(cmd: NetworkCommands) -> Result<()> {
    match cmd {
        NetworkCommands::Create {
            name,
            driver,
            subnet,
            gateway,
        } => cmd_network_create(name, driver, subnet, gateway).await,
        NetworkCommands::Ls { format } => cmd_network_ls(format).await,
        NetworkCommands::Inspect { networks, format } => {
            cmd_network_inspect(networks, format).await
        }
        NetworkCommands::Rm { networks } => cmd_network_rm(networks).await,
        NetworkCommands::Connect {
            network,
            container,
            ip,
        } => cmd_network_connect(network, container, ip).await,
        NetworkCommands::Disconnect { network, container } => {
            cmd_network_disconnect(network, container).await
        }
    }
}

async fn execute_volume_command(cmd: VolumeCommands) -> Result<()> {
    match cmd {
        VolumeCommands::Create { name, driver, opts } => {
            cmd_volume_create(name, driver, opts).await
        }
        VolumeCommands::Ls { format } => cmd_volume_ls(format).await,
        VolumeCommands::Inspect { volumes, format } => cmd_volume_inspect(volumes, format).await,
        VolumeCommands::Rm { volumes, force } => cmd_volume_rm(volumes, force).await,
        VolumeCommands::Prune { force } => cmd_volume_prune(force).await,
    }
}

async fn cmd_network_create(
    name: String,
    driver: String,
    subnet: Option<String>,
    gateway: Option<String>,
) -> Result<()> {
    let mut core = proot_distro_core::ProotDistroCore::new()?;
    let config = NetworkConfig {
        name: name.clone(),
        driver,
        subnet,
        gateway,
        dns_servers: vec!["8.8.8.8".to_string(), "8.8.4.4".to_string()],
        enable_ipv6: false,
        mtu: None,
        ..Default::default()
    };
    core.network_manager().create(&config)?;
    println!("Network '{}' created successfully", name);
    Ok(())
}

async fn cmd_network_ls(format: OutputFormat) -> Result<()> {
    let mut core = proot_distro_core::ProotDistroCore::new()?;
    let networks = core.network_manager().list()?;

    #[derive(Tabled, Serialize)]
    struct NetworkRow {
        #[tabled(rename = "NAME")]
        name: String,
        #[tabled(rename = "DRIVER")]
        driver: String,
        #[tabled(rename = "SUBNET")]
        subnet: String,
        #[tabled(rename = "GATEWAY")]
        gateway: String,
    }

    let rows: Vec<NetworkRow> = networks
        .iter()
        .map(|n| NetworkRow {
            name: n.name.clone(),
            driver: n.driver.clone(),
            subnet: n.subnet.clone().unwrap_or_default(),
            gateway: n.gateway.clone().unwrap_or_default(),
        })
        .collect();

    match format {
        OutputFormat::Table => output_table(&rows),
        OutputFormat::Json => output_json(&networks)?,
        OutputFormat::Yaml => output_yaml(&networks)?,
        OutputFormat::Text => {
            for n in networks {
                println!(
                    "{} {} {} {}",
                    n.name,
                    n.driver,
                    n.subnet.unwrap_or_default(),
                    n.gateway.unwrap_or_default()
                );
            }
        }
    }
    Ok(())
}

async fn cmd_network_inspect(networks: Vec<String>, format: OutputFormat) -> Result<()> {
    let mut core = proot_distro_core::ProotDistroCore::new()?;
    let mut results = Vec::new();

    for name in networks {
        match core.network_manager().inspect(&name) {
            Ok(info) => results.push(info),
            Err(e) => eprintln!("Network '{}': {}", name, e),
        }
    }

    #[derive(Tabled, Serialize)]
    struct NetworkRow {
        #[tabled(rename = "NAME")]
        name: String,
        #[tabled(rename = "DRIVER")]
        driver: String,
        #[tabled(rename = "SUBNET")]
        subnet: String,
        #[tabled(rename = "GATEWAY")]
        gateway: String,
        #[tabled(rename = "IPV6")]
        ipv6: String,
        #[tabled(rename = "MTU")]
        mtu: String,
    }

    let rows: Vec<NetworkRow> = results
        .iter()
        .map(|n| NetworkRow {
            name: n.name.clone(),
            driver: n.driver.clone(),
            subnet: n.subnet.clone().unwrap_or_default(),
            gateway: n.gateway.clone().unwrap_or_default(),
            ipv6: n.enable_ipv6.to_string(),
            mtu: n.mtu.map_or("".to_string(), |v| v.to_string()),
        })
        .collect();

    match format {
        OutputFormat::Table => output_table(&rows),
        OutputFormat::Json => output_json(&results)?,
        OutputFormat::Yaml => output_yaml(&results)?,
        OutputFormat::Text => {
            for n in results {
                println!(
                    "{} {} {} {}",
                    n.name,
                    n.driver,
                    n.subnet.unwrap_or_default(),
                    n.gateway.unwrap_or_default()
                );
            }
        }
    }
    Ok(())
}

async fn cmd_network_rm(networks: Vec<String>) -> Result<()> {
    let mut core = proot_distro_core::ProotDistroCore::new()?;

    for name in networks {
        match core.network_manager().remove(&name) {
            Ok(_) => println!("Removed network: {}", name),
            Err(e) => eprintln!("Failed to remove network {}: {}", name, e),
        }
    }
    Ok(())
}

/// Rename a container: the directory name *is* the identity, so the move plus a
/// rewritten `metadata.json` is the whole operation.
async fn cmd_rename(oldname: String, newname: String) -> Result<()> {
    if oldname == newname {
        anyhow::bail!("Source and destination names are identical");
    }
    proot_distro_core::NameValidator::new().validate_container_name(&newname)?;

    let mgr = proot_distro_core::ContainerManager::new()?;
    let container = mgr.get(&oldname).await?;

    let src_dir = container
        .rootfs_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("unexpected rootfs layout"))?
        .to_path_buf();
    let containers_dir = src_dir
        .parent()
        .ok_or_else(|| anyhow::anyhow!("unexpected container layout"))?
        .to_path_buf();
    let dst_dir = containers_dir.join(&newname);

    if dst_dir.exists() {
        anyhow::bail!("Container '{}' already exists", newname);
    }

    std::fs::rename(&src_dir, &dst_dir)?;

    // metadata.json embeds both the name and the rootfs path; a stale copy makes
    // `list` disagree with the directory on disk.
    let meta_path = dst_dir.join("metadata.json");
    if let Ok(raw) = std::fs::read_to_string(&meta_path) {
        if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&raw) {
            if let Some(obj) = value.as_object_mut() {
                obj.insert("name".into(), serde_json::json!(newname));
                obj.insert(
                    "rootfs".into(),
                    serde_json::json!(dst_dir.join("rootfs").to_string_lossy()),
                );
            }
            std::fs::write(&meta_path, serde_json::to_string_pretty(&value)?)?;
        }
    }

    // Rewrite any link2symlink intermediate targets pointing to old rootfs
    let old_rootfs = src_dir.join("rootfs");
    let new_rootfs = dst_dir.join("rootfs");
    if new_rootfs.exists() {
        if let Ok(rewritten) =
            proot_distro_core::l2s::rewrite_l2s_tree(&new_rootfs, &old_rootfs, &new_rootfs)
        {
            if rewritten > 0 {
                pn_trace!(
                    "Rewrote {} link2symlink target(s) for renamed container",
                    rewritten
                );
            }
        }
    }

    println!("Renamed container '{}' to '{}'", oldname, newname);
    Ok(())
}

/// Split `[CONTAINER:]PATH`. A prefix is only a container when it has no path
/// separator, so an absolute host path is never mistaken for one.
fn split_spec(spec: &str) -> (Option<&str>, &str) {
    match spec.split_once(':') {
        Some((name, path)) if !name.is_empty() && !name.contains('/') => (Some(name), path),
        _ => (None, spec),
    }
}

/// Translate a `[CONTAINER:]PATH` spec to a host path.
///
/// The rootfs is a plain host directory, so a guest path is just the rootfs
/// joined with the guest path — no protocol needed. But it is a **trust
/// boundary**: the guest path comes from the user, and a naive `join` lets
/// `c1:../../../../etc/shadow` walk straight out of the container and read a
/// host file. Every component is therefore checked, and the result is confirmed
/// to still be inside the rootfs after symlinks are resolved.
async fn resolve_endpoint(spec: &str) -> Result<std::path::PathBuf> {
    match split_spec(spec) {
        (None, path) => Ok(std::path::PathBuf::from(path)),
        (Some(name), path) => {
            let mgr = proot_distro_core::ContainerManager::new()?;
            let container = mgr.get(name).await?;
            guest_to_host(&container.rootfs_path, path)
        }
    }
}

/// Join a guest path onto a rootfs, refusing anything that leaves it.
fn guest_to_host(rootfs: &std::path::Path, guest_path: &str) -> Result<std::path::PathBuf> {
    use std::path::Component;

    let mut resolved = rootfs.to_path_buf();
    for component in std::path::Path::new(guest_path).components() {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::Normal(part) => resolved.push(part),
            // `..` is refused outright rather than normalised: inside a container
            // it can only ever mean "leave", and silently clamping it would make
            // the resulting path differ from what the user asked for.
            Component::ParentDir => {
                anyhow::bail!(
                    "path '{}' escapes the container rootfs (contains '..')",
                    guest_path
                )
            }
            Component::Prefix(_) => {
                anyhow::bail!("unsupported path component in '{}'", guest_path)
            }
        }
    }

    // Lexical checks cannot see a symlink inside the rootfs that points outside
    // it, so confirm against the real path when the target exists.
    if let (Ok(root), Ok(real)) = (
        std::fs::canonicalize(rootfs),
        std::fs::canonicalize(&resolved),
    ) {
        if !real.starts_with(&root) {
            anyhow::bail!(
                "path '{}' resolves outside the container rootfs ({})",
                guest_path,
                real.display()
            );
        }
    }

    Ok(resolved)
}

async fn cmd_copy(source: String, destination: String, recursive: bool, move_: bool) -> Result<()> {
    let dst = resolve_endpoint(&destination).await?;
    let raw_src = resolve_endpoint(&source).await?;

    // A plain copy follows the final symlink, exactly as `cp` does; `--move`
    // acts on the link itself. Without this, copying a guest path that happens
    // to be a link produces a dangling link on the host: Debian's
    // /etc/os-release is `../usr/lib/os-release`, which stops meaning anything
    // once it is outside the rootfs.
    let src_is_link = std::fs::symlink_metadata(&raw_src)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false);
    let src = if !move_ && src_is_link {
        let link = std::fs::read_link(&raw_src)?;
        if link.is_absolute() {
            link
        } else {
            raw_src
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .join(link)
        }
    } else {
        raw_src.clone()
    };

    let meta = std::fs::symlink_metadata(&src)
        .map_err(|_| anyhow::anyhow!("Source '{}' does not exist", source))?;
    let is_dir = meta.is_dir();
    if !is_dir && !meta.file_type().is_file() && !meta.file_type().is_symlink() {
        anyhow::bail!("Source '{}' is not a regular file or directory", source);
    }
    if is_dir && !recursive {
        anyhow::bail!("Source '{}' is a directory; pass --recursive", source);
    }

    // Copying onto an existing directory places the source inside it, as cp does.
    let dest = if dst.is_dir() {
        let name = src
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("source has no file name"))?;
        dst.join(name)
    } else {
        dst
    };

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }

    if is_dir {
        copy_dir_recursive(&src, &dest)?;
    } else if meta.file_type().is_symlink() {
        // Only reachable with --move: the leaf link is moved as a link.
        let link = std::fs::read_link(&src)?;
        let _ = std::fs::remove_file(&dest);
        std::os::unix::fs::symlink(&link, &dest)?;
    } else {
        std::fs::copy(&src, &dest)?;
    }

    if move_ {
        if is_dir {
            std::fs::remove_dir_all(&src)?;
        } else {
            std::fs::remove_file(&src)?;
        }
        println!("Moved {} -> {}", source, destination);
    } else {
        println!("Copied {} -> {}", source, destination);
    }
    Ok(())
}

/// Reset a container: remove it and reinstall the image it came from.
///
/// This is destructive by definition — the same contract as proot-distro's
/// `reset` — so it says so before doing it, and it refuses to guess an image it
/// cannot prove the container came from.
async fn cmd_reset(name: String) -> Result<()> {
    let mgr = proot_distro_core::ContainerManager::new()?;
    let container = mgr.get(&name).await?;

    let container_dir = container
        .rootfs_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("unexpected rootfs layout"))?
        .to_path_buf();

    let meta_path = container_dir.join("metadata.json");
    let meta: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&meta_path)?)?;
    let image = meta["image"].as_str().unwrap_or_default().to_string();
    if image.is_empty() {
        anyhow::bail!(
            "Container '{}' has no recorded source image, so there is nothing to reinstall. \
             Reinstall it manually.",
            name
        );
    }
    // A restored container records the *container* it came from as its image;
    // that is not a registry reference and must not be sent to a registry.
    let tag = meta["tag"].as_str().unwrap_or_default();
    let image_ref = if tag.is_empty() || tag == "restored" {
        image.clone()
    } else {
        format!("{}:{}", image, tag)
    };

    println!("Resetting '{}' from '{}'.", name, image_ref);
    println!("This deletes the container and everything inside it.");

    // Fail before the irreversible step. A `backup restore` records the *source
    // container* as the image, which is not a registry reference; the old order
    // deleted the container first and only then discovered the image could not be
    // fetched, losing the container with nothing to replace it.
    verify_image_available(&image_ref).await?;

    std::fs::remove_dir_all(&container_dir)?;
    install_image_native(&image_ref, &name, false, None).await?;

    println!("\nContainer '{}' reset to a clean {}", name, image_ref);
    Ok(())
}

/// Counters for one sync run, so the command reports what it actually did.
#[derive(Default)]
struct SyncReport {
    copied: usize,
    unchanged: usize,
    removed: usize,
    skipped: usize,
}

/// Mirror `source` onto `destination`.
///
/// Comparison is by content (size first, then sha256), not by mtime: PDN has no
/// dependency that can set a destination's mtime back, so an mtime comparison
/// would report every file as changed on every run. Hashing costs more and is
/// the only honest option here.
async fn cmd_sync(source: String, destination: String, delete: bool) -> Result<()> {
    let src = resolve_endpoint(&source).await?;
    let dst = resolve_endpoint(&destination).await?;

    let src_meta = std::fs::symlink_metadata(&src)
        .map_err(|_| anyhow::anyhow!("Source '{}' does not exist", source))?;

    // Without this, `--delete` against a file source silently does nothing,
    // which is the bug the reference implementation documents avoiding.
    if delete && !src_meta.is_dir() {
        anyhow::bail!("--delete requires a directory as the source");
    }

    let mut report = SyncReport::default();

    if src_meta.is_dir() {
        mirror_dir(&src, &dst, &mut report)?;
        if delete {
            prune_extras(&src, &dst, &mut report)?;
        }
    } else {
        sync_one(&src, &dst, &mut report)?;
    }

    println!(
        "Synced {} -> {}: {} copied, {} unchanged, {} removed, {} skipped",
        source, destination, report.copied, report.unchanged, report.removed, report.skipped
    );
    Ok(())
}

fn file_digest(path: &std::path::Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    let mut file = std::fs::File::open(path)?;
    std::io::copy(&mut file, &mut hasher)?;
    Ok(hex::encode(hasher.finalize()))
}

/// True when the destination already holds this content.
fn same_content(src: &std::path::Path, dst: &std::path::Path) -> Result<bool> {
    let s = std::fs::symlink_metadata(src)?;
    let Ok(d) = std::fs::symlink_metadata(dst) else {
        return Ok(false);
    };
    if !d.is_file() || !s.is_file() {
        return Ok(false);
    }
    if s.len() != d.len() {
        return Ok(false);
    }
    Ok(file_digest(src)? == file_digest(dst)?)
}

/// Sync a single file, placing it inside `dst` when `dst` is a directory.
fn sync_one(src: &std::path::Path, dst: &std::path::Path, report: &mut SyncReport) -> Result<()> {
    let meta = std::fs::symlink_metadata(src)?;

    let dst_is_dir = std::fs::symlink_metadata(dst)
        .map(|m| m.is_dir())
        .unwrap_or(false);
    let target = if dst_is_dir {
        let name = src
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("source has no file name"))?;
        dst.join(name)
    } else {
        dst.to_path_buf()
    };

    if meta.file_type().is_symlink() {
        let link = std::fs::read_link(src)?;
        let already = std::fs::read_link(&target).ok().as_deref() == Some(link.as_path());
        if already {
            report.unchanged += 1;
            return Ok(());
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let _ = std::fs::remove_file(&target);
        std::os::unix::fs::symlink(&link, &target)?;
        report.copied += 1;
        return Ok(());
    }

    // A directory standing where the source has a file is replaced, never
    // descended into: that link may lead somewhere unexpected.
    if std::fs::symlink_metadata(&target)
        .map(|m| m.is_dir())
        .unwrap_or(false)
    {
        std::fs::remove_dir_all(&target)?;
    }

    if same_content(src, &target)? {
        report.unchanged += 1;
        return Ok(());
    }

    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(src, &target)?;
    std::fs::set_permissions(&target, meta.permissions())?;
    report.copied += 1;
    Ok(())
}

/// Walk `src` and mirror every entry into `dst`. Ownership is never changed
/// (an unprivileged Android process cannot), and modes are applied after the
/// content so a read-only source directory stays writable while its children go
/// in.
fn mirror_dir(src: &std::path::Path, dst: &std::path::Path, report: &mut SyncReport) -> Result<()> {
    std::fs::create_dir_all(dst)?;

    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        let target = dst.join(entry.file_name());

        if file_type.is_dir() {
            mirror_dir(&path, &target, report)?;
        } else {
            sync_one(&path, &target, report)?;
        }
    }

    if let Ok(meta) = std::fs::metadata(src) {
        let _ = std::fs::set_permissions(dst, meta.permissions());
    }
    Ok(())
}

/// Remove destination entries that have no counterpart in the source.
///
/// Only meaningful after a successful mirror pass; the caller guarantees the
/// source directory was listable, or every destination entry would look like an
/// orphan.
fn prune_extras(
    src: &std::path::Path,
    dst: &std::path::Path,
    report: &mut SyncReport,
) -> Result<()> {
    let Ok(entries) = std::fs::read_dir(dst) else {
        return Ok(());
    };

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let counterpart = src.join(entry.file_name());

        if std::fs::symlink_metadata(&counterpart).is_err() {
            if entry.file_type()?.is_dir() {
                std::fs::remove_dir_all(&path)?;
            } else {
                std::fs::remove_file(&path)?;
            }
            report.removed += 1;
            continue;
        }

        if entry.file_type()?.is_dir() {
            prune_extras(&counterpart, &path, report)?;
        }
    }
    Ok(())
}

/// Recursive copy that keeps symlinks as symlinks, so a copied tree does not
/// silently balloon the way following every link would.
fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;

    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let target = dst.join(entry.file_name());

        if file_type.is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else if file_type.is_symlink() {
            let link = std::fs::read_link(entry.path())?;
            let _ = std::fs::remove_file(&target);
            std::os::unix::fs::symlink(&link, &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }

    Ok(())
}

async fn cmd_network_connect(network: String, container: String, ip: Option<String>) -> Result<()> {
    // Resolve the container first: an unknown name must fail before any state
    // is written, and we need its rootfs to apply the network's DNS.
    let container_mgr = proot_distro_core::ContainerManager::new()?;
    let target = container_mgr.get(&container).await?;

    let mut core = proot_distro_core::ProotDistroCore::new()?;
    core.network_manager().connect(
        &network,
        &container,
        ip.as_deref(),
        Some(target.rootfs_path.as_path()),
    )?;

    println!("Connected container {} to network {}", container, network);
    if let Some(ip) = ip {
        println!("IP: {}", ip);
    }
    Ok(())
}

async fn cmd_network_disconnect(network: String, container: String) -> Result<()> {
    let container_mgr = proot_distro_core::ContainerManager::new()?;
    let target = container_mgr.get(&container).await?;

    let mut core = proot_distro_core::ProotDistroCore::new()?;
    core.network_manager()
        .disconnect(&network, &container, Some(target.rootfs_path.as_path()))?;

    println!(
        "Disconnected container {} from network {}",
        container, network
    );
    Ok(())
}

async fn cmd_volume_create(name: String, driver: String, opts: Vec<String>) -> Result<()> {
    let mut core = proot_distro_core::ProotDistroCore::new()?;
    let labels = std::collections::HashMap::new();
    let mut options = std::collections::HashMap::new();

    for opt in opts {
        if let Some((k, v)) = opt.split_once('=') {
            options.insert(k.to_string(), v.to_string());
        }
    }

    let config = VolumeConfig {
        name: name.clone(),
        driver,
        labels,
        options,
    };
    core.storage_manager().create_volume(&config)?;
    println!("Volume '{}' created successfully", name);
    Ok(())
}

async fn cmd_volume_ls(format: OutputFormat) -> Result<()> {
    let mut core = proot_distro_core::ProotDistroCore::new()?;
    let volumes = core.storage_manager().list_volumes()?;

    #[derive(Tabled, Serialize)]
    struct VolumeRow {
        #[tabled(rename = "NAME")]
        name: String,
        #[tabled(rename = "DRIVER")]
        driver: String,
        #[tabled(rename = "MOUNTPOINT")]
        mountpoint: String,
    }

    let rows: Vec<VolumeRow> = volumes
        .iter()
        .map(|v| VolumeRow {
            name: v.name.clone(),
            driver: v.driver.clone(),
            mountpoint: v.mountpoint.display().to_string(),
        })
        .collect();

    match format {
        OutputFormat::Table => output_table(&rows),
        OutputFormat::Json => output_json(&volumes)?,
        OutputFormat::Yaml => output_yaml(&volumes)?,
        OutputFormat::Text => {
            for v in volumes {
                println!("{} {} {}", v.name, v.driver, v.mountpoint.display());
            }
        }
    }
    Ok(())
}

async fn cmd_volume_inspect(volumes: Vec<String>, format: OutputFormat) -> Result<()> {
    let mut core = proot_distro_core::ProotDistroCore::new()?;
    let mut results = Vec::new();

    for name in volumes {
        match core.storage_manager().inspect_volume(&name) {
            Ok(info) => results.push(info),
            Err(e) => eprintln!("Volume '{}': {}", name, e),
        }
    }

    #[derive(Tabled, Serialize)]
    struct VolumeRow {
        #[tabled(rename = "NAME")]
        name: String,
        #[tabled(rename = "DRIVER")]
        driver: String,
        #[tabled(rename = "MOUNTPOINT")]
        mountpoint: String,
        #[tabled(rename = "SIZE")]
        size: String,
    }

    let rows: Vec<VolumeRow> = results
        .iter()
        .map(|v| VolumeRow {
            name: v.name.clone(),
            driver: v.driver.clone(),
            mountpoint: v.mountpoint.display().to_string(),
            size: v.size.to_string(),
        })
        .collect();

    match format {
        OutputFormat::Table => output_table(&rows),
        OutputFormat::Json => output_json(&results)?,
        OutputFormat::Yaml => output_yaml(&results)?,
        OutputFormat::Text => {
            for v in results {
                println!("{} {} {}", v.name, v.driver, v.mountpoint.display());
            }
        }
    }
    Ok(())
}

async fn cmd_volume_rm(volumes: Vec<String>, force: bool) -> Result<()> {
    let mut core = proot_distro_core::ProotDistroCore::new()?;

    for name in volumes {
        if !force {
            print!("Remove volume '{}'? [y/N]: ", name);
            io::stdout().flush()?;
            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            if !input.trim().eq_ignore_ascii_case("y") {
                println!("Cancelled.");
                continue;
            }
        }
        match core.storage_manager().remove_volume(&name) {
            Ok(_) => println!("Removed volume: {}", name),
            Err(e) => eprintln!("Failed to remove volume {}: {}", name, e),
        }
    }
    Ok(())
}

async fn cmd_volume_prune(force: bool) -> Result<()> {
    use proot_distro_core::ContainerManager;
    use std::path::PathBuf;

    // A volume is "in use" when some container mounts a path inside it. The
    // container config is the only place that records this, so gather the mount
    // sources before touching anything.
    let container_mgr = ContainerManager::new()?;
    let mut mounted: Vec<PathBuf> = Vec::new();
    for info in container_mgr.list().await? {
        if let Ok(c) = container_mgr.get(&info.name).await {
            for b in &c.config.bind_mounts {
                mounted.push(PathBuf::from(&b.source));
            }
        }
    }

    let mut core = proot_distro_core::ProotDistroCore::new()?;

    // Decide the work set first, so the confirmation prompt lists exactly what
    // will be deleted rather than re-scanning afterwards.
    let mut unused: Vec<String> = Vec::new();
    for v in core.storage_manager().list_volumes()? {
        let used = mounted.iter().any(|m| m.starts_with(&v.mountpoint));
        if used {
            println!("Skipping in-use volume: {}", v.name);
        } else {
            unused.push(v.name);
        }
    }

    if unused.is_empty() {
        println!("No unused volumes to prune.");
        return Ok(());
    }

    if !force {
        println!(
            "Will remove {} volume(s): {}",
            unused.len(),
            unused.join(", ")
        );
        print!("Continue? [y/N]: ");
        std::io::Write::flush(&mut std::io::stdout())?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Cancelled.");
            return Ok(());
        }
    }

    let mut pruned = 0usize;
    for name in &unused {
        match core.storage_manager().remove_volume(name) {
            Ok(_) => {
                println!("Pruned volume: {}", name);
                pruned += 1;
            }
            Err(e) => eprintln!("Failed to prune volume {}: {}", name, e),
        }
    }
    println!("Pruned {} volume(s)", pruned);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The guest path in `container:[CONTAINER:]PATH` comes from the user, so it
    /// is a trust boundary. A plain `join` let `c1:../../../../etc/shadow` read a
    /// host file; these cases pin that shut.
    #[test]
    fn guest_to_host_refuses_traversal() {
        let rootfs = std::path::Path::new("/data/podnext/containers/c1/rootfs");

        // Ordinary paths resolve under the rootfs.
        assert_eq!(
            guest_to_host(rootfs, "/etc/hosts").unwrap(),
            rootfs.join("etc/hosts")
        );
        assert_eq!(
            guest_to_host(rootfs, "etc/hosts").unwrap(),
            rootfs.join("etc/hosts")
        );
        // "." is a no-op, and a trailing slash is fine.
        assert_eq!(
            guest_to_host(rootfs, "/./etc/hosts").unwrap(),
            rootfs.join("etc/hosts")
        );

        // Anything that climbs out is refused, however it is spelled.
        for evil in [
            "/../../../../etc/shadow",
            "../etc/shadow",
            "etc/../../..",
            "/etc/../../../etc/shadow",
            "..",
            "/..",
        ] {
            assert!(
                guest_to_host(rootfs, evil).is_err(),
                "expected {:?} to be refused",
                evil
            );
        }
    }

    fn tmp_tree(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("pdn-sync-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn sync_transfers_only_what_differs() {
        let root = tmp_tree("diff");
        let src = root.join("src");
        let dst = root.join("dst");
        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::write(src.join("a.txt"), "one").unwrap();
        std::fs::write(src.join("sub/b.txt"), "two").unwrap();

        let mut report = SyncReport::default();
        mirror_dir(&src, &dst, &mut report).unwrap();
        assert_eq!(report.copied, 2, "first pass copies everything");
        assert_eq!(
            std::fs::read_to_string(dst.join("sub/b.txt")).unwrap(),
            "two"
        );

        // Second pass: nothing changed, nothing moves.
        let mut second = SyncReport::default();
        mirror_dir(&src, &dst, &mut second).unwrap();
        assert_eq!(second.copied, 0);
        assert_eq!(second.unchanged, 2);

        // Change one file and only that one moves.
        std::fs::write(src.join("a.txt"), "ONE!").unwrap();
        let mut third = SyncReport::default();
        mirror_dir(&src, &dst, &mut third).unwrap();
        assert_eq!(third.copied, 1);
        assert_eq!(third.unchanged, 1);
        assert_eq!(std::fs::read_to_string(dst.join("a.txt")).unwrap(), "ONE!");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn prune_removes_only_orphans() {
        let root = tmp_tree("prune");
        let src = root.join("src");
        let dst = root.join("dst");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(dst.join("stale-dir")).unwrap();
        std::fs::write(src.join("keep.txt"), "k").unwrap();
        std::fs::write(dst.join("keep.txt"), "k").unwrap();
        std::fs::write(dst.join("drop.txt"), "d").unwrap();
        std::fs::write(dst.join("stale-dir/inner.txt"), "d").unwrap();

        let mut report = SyncReport::default();
        prune_extras(&src, &dst, &mut report).unwrap();

        assert!(dst.join("keep.txt").exists(), "a live file must survive");
        assert!(!dst.join("drop.txt").exists(), "an orphan must go");
        assert!(
            !dst.join("stale-dir").exists(),
            "an orphan directory must go"
        );
        assert_eq!(report.removed, 2);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn same_content_compares_bytes_not_names() {
        let root = tmp_tree("same");
        std::fs::write(root.join("a"), "abcdef").unwrap();
        std::fs::write(root.join("b"), "abcdef").unwrap();
        std::fs::write(root.join("c"), "abcde").unwrap();

        assert!(same_content(&root.join("a"), &root.join("b")).unwrap());
        assert!(!same_content(&root.join("a"), &root.join("c")).unwrap());
        assert!(!same_content(&root.join("a"), &root.join("missing")).unwrap());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn split_spec_only_treats_a_path_free_prefix_as_a_container() {
        assert_eq!(split_spec("c1:/etc/x"), (Some("c1"), "/etc/x"));
        assert_eq!(split_spec("/etc/x"), (None, "/etc/x"));
        // A host path containing a colon must not be read as a container name.
        assert_eq!(split_spec("/data/a:b/c"), (None, "/data/a:b/c"));
        assert_eq!(split_spec("./rel/x"), (None, "./rel/x"));
    }
}
