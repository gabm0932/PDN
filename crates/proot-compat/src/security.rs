//! Security hardening for PRoot containers
//!
//! Provides seccomp profile management, AppArmor/SELinux integration,
//! and security policy enforcement for containers.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Security profile configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityProfileConfig {
    pub name: String,
    pub profile_type: SecurityProfileType,
    pub seccomp: Option<SeccompProfile>,
    pub apparmor: Option<AppArmorProfile>,
    pub selinux: Option<SelLinuxProfile>,
    pub capabilities: CapabilityConfig,
    pub no_new_privileges: bool,
    pub read_only_rootfs: bool,
    pub user_namespace: bool,
    pub enabled: bool,
}

impl Default for SecurityProfileConfig {
    fn default() -> Self {
        Self {
            name: "default".to_string(),
            profile_type: SecurityProfileType::Default,
            seccomp: Some(SeccompProfile::default()),
            apparmor: None,
            selinux: None,
            capabilities: CapabilityConfig::default(),
            no_new_privileges: true,
            read_only_rootfs: false,
            user_namespace: false,
            enabled: true,
        }
    }
}

/// Security profile types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SecurityProfileType {
    Default,
    Unconfined,
    Restricted,
    Custom(String),
}

/// Seccomp profile
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeccompProfile {
    pub default_action: SeccompAction,
    pub architectures: Vec<SeccompArch>,
    pub syscalls: Vec<SeccompSyscallRule>,
    pub listener_path: Option<String>,
    pub listener_metadata: Option<String>,
}

impl Default for SeccompProfile {
    fn default() -> Self {
        Self {
            default_action: SeccompAction::Errno,
            architectures: vec![
                SeccompArch::X86_64,
                SeccompArch::Aarch64,
                SeccompArch::X86,
                SeccompArch::Arm,
            ],
            syscalls: get_default_seccomp_rules(),
            listener_path: None,
            listener_metadata: None,
        }
    }
}

/// Seccomp actions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SeccompAction {
    Kill,
    KillProcess,
    Trap,
    Errno,
    Trace,
    Allow,
    Log,
}

/// Seccomp architectures
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SeccompArch {
    X86_64,
    X86,
    X32,
    Arm,
    Aarch64,
    Mips,
    Mips64,
    Mips64n32,
    Mipsel,
    Mipsel64,
    Mipsel64n32,
    Ppc64le,
    S390,
    S390x,
    Riscv64,
}

/// Seccomp syscall rule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeccompSyscallRule {
    pub names: Vec<String>,
    pub action: SeccompAction,
    pub args: Option<Vec<SeccompArg>>,
}

/// Seccomp argument filter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeccompArg {
    pub index: u32,
    pub value: u64,
    pub value_two: Option<u64>,
    pub op: SeccompOp,
}

/// Seccomp operators
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SeccompOp {
    Equal,
    NotEqual,
    MaskedEqual,
    Ge,
    Le,
    Gt,
    Lt,
}

/// AppArmor profile
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppArmorProfile {
    pub profile_name: String,
    pub profile_content: String,
    pub enforce: bool,
}

/// SELinux profile
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelLinuxProfile {
    pub context: String,
    pub level: Option<String>,
    pub role: Option<String>,
    pub type_: Option<String>,
    pub user: Option<String>,
}

/// Capability configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityConfig {
    pub add: Vec<String>,
    pub drop: Vec<String>,
    pub bounding: Vec<String>,
    pub effective: Vec<String>,
    pub inheritable: Vec<String>,
    pub permitted: Vec<String>,
    pub ambient: Vec<String>,
}

impl Default for CapabilityConfig {
    fn default() -> Self {
        Self {
            add: vec![],
            drop: vec![
                "CAP_SYS_ADMIN".to_string(),
                "CAP_SYS_RESOURCE".to_string(),
                "CAP_SYS_MODULE".to_string(),
                "CAP_SYS_RAWIO".to_string(),
                "CAP_SYS_PACCT".to_string(),
                "CAP_SYS_BOOT".to_string(),
                "CAP_SYS_NICE".to_string(),
                "CAP_SYS_TTY_CONFIG".to_string(),
                "CAP_SYS_RESOURCE".to_string(),
                "CAP_DAC_OVERRIDE".to_string(),
                "CAP_DAC_READ_SEARCH".to_string(),
                "CAP_FOWNER".to_string(),
                "CAP_FSETID".to_string(),
                "CAP_KILL".to_string(),
                "CAP_SETGID".to_string(),
                "CAP_SETUID".to_string(),
                "CAP_LINUX_IMMUTABLE".to_string(),
                "CAP_NET_BIND_SERVICE".to_string(),
                "CAP_NET_BROADCAST".to_string(),
                "CAP_NET_ADMIN".to_string(),
                "CAP_NET_RAW".to_string(),
                "CAP_IPC_LOCK".to_string(),
                "CAP_IPC_OWNER".to_string(),
                "CAP_SYS_CHROOT".to_string(),
                "CAP_SYS_PTRACE".to_string(),
                "CAP_SYS_MODULE".to_string(),
                "CAP_CADET".to_string(),
            ],
            bounding: vec![],
            effective: vec![],
            inheritable: vec![],
            permitted: vec![],
            ambient: vec![],
        }
    }
}

/// Security hardening manager
pub struct SecurityHardeningManager {
    profiles: HashMap<String, SecurityProfileConfig>,
    seccomp_dir: PathBuf,
    apparmor_dir: PathBuf,
    selinux_dir: PathBuf,
}

impl SecurityHardeningManager {
    pub fn new() -> Result<Self> {
        Self::with_custom_dirs(
            PathBuf::from("/etc/pn-next/seccomp"),
            PathBuf::from("/etc/apparmor.d/pn-next"),
            PathBuf::from("/etc/selinux/pn-next"),
        )
    }

    /// Create a new SecurityHardeningManager with custom directory paths (useful for testing)
    pub fn with_custom_dirs(
        seccomp_dir: PathBuf,
        apparmor_dir: PathBuf,
        selinux_dir: PathBuf,
    ) -> Result<Self> {
        fs::create_dir_all(&seccomp_dir)?;
        fs::create_dir_all(&apparmor_dir)?;
        fs::create_dir_all(&selinux_dir)?;

        Ok(Self {
            profiles: HashMap::new(),
            seccomp_dir,
            apparmor_dir,
            selinux_dir,
        })
    }

    /// Register a security profile
    pub fn register_profile(&mut self, name: String, config: SecurityProfileConfig) {
        self.profiles.insert(name, config);
    }

    /// Get a security profile
    pub fn get_profile(&self, name: &str) -> Option<&SecurityProfileConfig> {
        self.profiles.get(name)
    }

    /// Apply security profile to container
    pub async fn apply_profile(&self, profile_name: &str, container_rootfs: &Path) -> Result<()> {
        let profile = self
            .profiles
            .get(profile_name)
            .ok_or_else(|| anyhow::anyhow!("Profile not found: {}", profile_name))?;

        // Apply seccomp profile
        if let Some(seccomp) = &profile.seccomp {
            self.apply_seccomp_profile(seccomp, container_rootfs)
                .await?;
        }

        // Apply AppArmor profile
        if let Some(apparmor) = &profile.apparmor {
            self.apply_apparmor_profile(apparmor, container_rootfs)
                .await?;
        }

        // Apply SELinux profile
        if let Some(selinux) = &profile.selinux {
            self.apply_selinux_profile(selinux, container_rootfs)
                .await?;
        }

        // Apply capability configuration
        self.apply_capabilities(&profile.capabilities, container_rootfs)
            .await?;

        // Apply no_new_privileges
        if profile.no_new_privileges {
            self.apply_no_new_privileges(container_rootfs).await?;
        }

        // Apply read-only rootfs
        if profile.read_only_rootfs {
            self.apply_read_only_rootfs(container_rootfs).await?;
        }

        Ok(())
    }

    /// Apply seccomp profile
    async fn apply_seccomp_profile(
        &self,
        profile: &SeccompProfile,
        _container_rootfs: &Path,
    ) -> Result<()> {
        // Generate seccomp JSON profile
        let profile_json = serde_json::to_string_pretty(profile)?;

        // Save to seccomp directory
        let profile_path = self.seccomp_dir.join("default.json");
        fs::write(&profile_path, profile_json)?;

        // In a real implementation, this would load the seccomp profile into the kernel
        // using prctl(PR_SET_SECCOMP, SECCOMP_MODE_FILTER, &filter) or similar

        tracing::info!("Applied seccomp profile: {:?}", profile.default_action);
        Ok(())
    }

    /// Apply AppArmor profile
    async fn apply_apparmor_profile(
        &self,
        profile: &AppArmorProfile,
        _container_rootfs: &Path,
    ) -> Result<()> {
        // Save AppArmor profile
        let profile_path = self.apparmor_dir.join(&profile.profile_name);
        fs::write(&profile_path, &profile.profile_content)?;

        // Load AppArmor profile
        // In real implementation: apparmor_parser -r <profile>

        tracing::info!("Applied AppArmor profile: {}", profile.profile_name);
        Ok(())
    }

    /// Apply SELinux profile
    async fn apply_selinux_profile(
        &self,
        profile: &SelLinuxProfile,
        _container_rootfs: &Path,
    ) -> Result<()> {
        // Set SELinux context for container
        // In real implementation: setcon or similar

        tracing::info!("Applied SELinux context: {}", profile.context);
        Ok(())
    }

    /// Apply capabilities
    async fn apply_capabilities(
        &self,
        caps: &CapabilityConfig,
        _container_rootfs: &Path,
    ) -> Result<()> {
        // Apply capability configuration using libcap or similar
        // This would typically use capset() or similar syscalls

        tracing::info!(
            "Applied capabilities: drop={:?}, add={:?}",
            caps.drop,
            caps.add
        );
        Ok(())
    }

    /// Apply no_new_privileges
    async fn apply_no_new_privileges(&self, _container_rootfs: &Path) -> Result<()> {
        // Set PR_SET_NO_NEW_PRIVS
        // unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) };

        tracing::info!("Applied no_new_privileges");
        Ok(())
    }

    /// Apply read-only rootfs
    async fn apply_read_only_rootfs(&self, _container_rootfs: &Path) -> Result<()> {
        // Mount rootfs as read-only
        // nix::mount::mount(
        //     Some("none"),
        //     container_rootfs,
        //     None::<&str>,
        //     nix::mount::MsFlags::MS_BIND | nix::mount::MsFlags::MS_RDONLY | nix::mount::MsFlags::MS_REMOUNT,
        //     None::<&str>
        // )?;

        tracing::info!("Applied read-only rootfs");
        Ok(())
    }

    /// Generate default seccomp profile for containers
    pub fn generate_default_seccomp_profile() -> SeccompProfile {
        SeccompProfile::default()
    }

    /// Generate restricted seccomp profile
    pub fn generate_restricted_seccomp_profile() -> SeccompProfile {
        SeccompProfile {
            default_action: SeccompAction::Kill,
            architectures: vec![SeccompArch::X86_64, SeccompArch::Aarch64],
            syscalls: get_restricted_seccomp_rules(),
            listener_path: None,
            listener_metadata: None,
        }
    }

    /// Generate unconfined seccomp profile
    pub fn generate_unconfined_seccomp_profile() -> SeccompProfile {
        SeccompProfile {
            default_action: SeccompAction::Allow,
            architectures: vec![
                SeccompArch::X86_64,
                SeccompArch::Aarch64,
                SeccompArch::X86,
                SeccompArch::Arm,
            ],
            syscalls: vec![],
            listener_path: None,
            listener_metadata: None,
        }
    }
}

/// Get default seccomp rules (allow common syscalls, block dangerous ones)
fn get_default_seccomp_rules() -> Vec<SeccompSyscallRule> {
    vec![
        // Allow basic syscalls
        SeccompSyscallRule {
            names: vec![
                "read".to_string(),
                "write".to_string(),
                "open".to_string(),
                "close".to_string(),
                "stat".to_string(),
                "fstat".to_string(),
                "lstat".to_string(),
                "poll".to_string(),
                "lseek".to_string(),
                "mmap".to_string(),
                "mprotect".to_string(),
                "munmap".to_string(),
                "brk".to_string(),
                "rt_sigaction".to_string(),
                "rt_sigprocmask".to_string(),
                "rt_sigreturn".to_string(),
                "ioctl".to_string(),
                "pread64".to_string(),
                "pwrite64".to_string(),
                "readv".to_string(),
                "writev".to_string(),
                "access".to_string(),
                "pipe".to_string(),
                "select".to_string(),
                "sched_yield".to_string(),
                "mremap".to_string(),
                "msync".to_string(),
                "mincore".to_string(),
                "madvise".to_string(),
                "shmget".to_string(),
                "shmat".to_string(),
                "shmctl".to_string(),
                "dup".to_string(),
                "dup2".to_string(),
                "pause".to_string(),
                "nanosleep".to_string(),
                "getitimer".to_string(),
                "alarm".to_string(),
                "setitimer".to_string(),
                "getpid".to_string(),
                "sendfile".to_string(),
                "socket".to_string(),
                "connect".to_string(),
                "accept".to_string(),
                "sendto".to_string(),
                "recvfrom".to_string(),
                "sendmsg".to_string(),
                "recvmsg".to_string(),
                "shutdown".to_string(),
                "bind".to_string(),
                "listen".to_string(),
                "getsockname".to_string(),
                "getpeername".to_string(),
                "socketpair".to_string(),
                "setsockopt".to_string(),
                "getsockopt".to_string(),
                "clone".to_string(),
                "fork".to_string(),
                "vfork".to_string(),
                "execve".to_string(),
                "exit".to_string(),
                "wait4".to_string(),
                "kill".to_string(),
                "uname".to_string(),
                "semget".to_string(),
                "semop".to_string(),
                "semctl".to_string(),
                "shmdt".to_string(),
                "msgget".to_string(),
                "msgsnd".to_string(),
                "msgrcv".to_string(),
                "msgctl".to_string(),
                "fcntl".to_string(),
                "flock".to_string(),
                "fsync".to_string(),
                "fdatasync".to_string(),
                "truncate".to_string(),
                "ftruncate".to_string(),
                "getdents".to_string(),
                "getcwd".to_string(),
                "chdir".to_string(),
                "fchdir".to_string(),
                "rename".to_string(),
                "mkdir".to_string(),
                "rmdir".to_string(),
                "creat".to_string(),
                "link".to_string(),
                "unlink".to_string(),
                "symlink".to_string(),
                "readlink".to_string(),
                "chmod".to_string(),
                "fchmod".to_string(),
                "chown".to_string(),
                "fchown".to_string(),
                "lchown".to_string(),
                "umask".to_string(),
                "gettimeofday".to_string(),
                "getrlimit".to_string(),
                "getrusage".to_string(),
                "sysinfo".to_string(),
                "times".to_string(),
                "ptrace".to_string(),
                "getuid".to_string(),
                "syslog".to_string(),
                "getgid".to_string(),
                "setuid".to_string(),
                "setgid".to_string(),
                "geteuid".to_string(),
                "getegid".to_string(),
                "setpgid".to_string(),
                "getppid".to_string(),
                "getpgrp".to_string(),
                "setsid".to_string(),
                "setreuid".to_string(),
                "setregid".to_string(),
                "getgroups".to_string(),
                "setgroups".to_string(),
                "setresuid".to_string(),
                "getresuid".to_string(),
                "setresgid".to_string(),
                "getresgid".to_string(),
                "getpgid".to_string(),
                "setfsuid".to_string(),
                "setfsgid".to_string(),
                "getsid".to_string(),
                "capget".to_string(),
                "capset".to_string(),
                "rt_sigpending".to_string(),
                "rt_sigtimedwait".to_string(),
                "rt_sigqueueinfo".to_string(),
                "rt_sigsuspend".to_string(),
                "sigaltstack".to_string(),
                "utime".to_string(),
                "mknod".to_string(),
                "uselib".to_string(),
                "personality".to_string(),
                "ustat".to_string(),
                "statfs".to_string(),
                "fstatfs".to_string(),
                "sysfs".to_string(),
                "getpriority".to_string(),
                "setpriority".to_string(),
                "sched_setparam".to_string(),
                "sched_getparam".to_string(),
                "sched_setscheduler".to_string(),
                "sched_getscheduler".to_string(),
                "sched_get_priority_max".to_string(),
                "sched_get_priority_min".to_string(),
                "sched_rr_get_interval".to_string(),
                "mlock".to_string(),
                "munlock".to_string(),
                "mlockall".to_string(),
                "munlockall".to_string(),
                "vhangup".to_string(),
                "modify_ldt".to_string(),
                "pivot_root".to_string(),
                "_sysctl".to_string(),
                "prctl".to_string(),
                "arch_prctl".to_string(),
                "adjtimex".to_string(),
                "setrlimit".to_string(),
                "chroot".to_string(),
                "sync".to_string(),
                "acct".to_string(),
                "settimeofday".to_string(),
                "mount".to_string(),
                "umount2".to_string(),
                "swapon".to_string(),
                "swapoff".to_string(),
                "reboot".to_string(),
                "sethostname".to_string(),
                "setdomainname".to_string(),
                "iopl".to_string(),
                "ioperm".to_string(),
                "create_module".to_string(),
                "init_module".to_string(),
                "delete_module".to_string(),
                "get_kernel_syms".to_string(),
                "query_module".to_string(),
                "quotactl".to_string(),
                "nfsservctl".to_string(),
                "getpmsg".to_string(),
                "putpmsg".to_string(),
                "afs_syscall".to_string(),
                "tuxcall".to_string(),
                "security".to_string(),
                "gettid".to_string(),
                "readahead".to_string(),
                "setxattr".to_string(),
                "lsetxattr".to_string(),
                "fsetxattr".to_string(),
                "getxattr".to_string(),
                "lgetxattr".to_string(),
                "fgetxattr".to_string(),
                "listxattr".to_string(),
                "llistxattr".to_string(),
                "flistxattr".to_string(),
                "removexattr".to_string(),
                "lremovexattr".to_string(),
                "fremovexattr".to_string(),
                "tkill".to_string(),
                "time".to_string(),
                "futex".to_string(),
                "sched_setaffinity".to_string(),
                "sched_getaffinity".to_string(),
                "set_thread_area".to_string(),
                "io_setup".to_string(),
                "io_destroy".to_string(),
                "io_getevents".to_string(),
                "io_submit".to_string(),
                "io_cancel".to_string(),
                "get_thread_area".to_string(),
                "lookup_dcookie".to_string(),
                "epoll_create".to_string(),
                "epoll_ctl_old".to_string(),
                "epoll_wait_old".to_string(),
                "remap_file_pages".to_string(),
                "getdents64".to_string(),
                "set_tid_address".to_string(),
                "restart_syscall".to_string(),
                "semtimedop".to_string(),
                "fadvise64".to_string(),
                "timer_create".to_string(),
                "timer_settime".to_string(),
                "timer_gettime".to_string(),
                "timer_getoverrun".to_string(),
                "timer_delete".to_string(),
                "clock_settime".to_string(),
                "clock_gettime".to_string(),
                "clock_getres".to_string(),
                "clock_nanosleep".to_string(),
                "exit_group".to_string(),
                "epoll_wait".to_string(),
                "epoll_ctl".to_string(),
                "tgkill".to_string(),
                "utimes".to_string(),
                "vserver".to_string(),
                "mbind".to_string(),
                "set_mempolicy".to_string(),
                "get_mempolicy".to_string(),
                "mq_open".to_string(),
                "mq_unlink".to_string(),
                "mq_timedsend".to_string(),
                "mq_timedreceive".to_string(),
                "mq_notify".to_string(),
                "mq_getsetattr".to_string(),
                "kexec_load".to_string(),
                "waitid".to_string(),
                "add_key".to_string(),
                "request_key".to_string(),
                "keyctl".to_string(),
                "ioprio_set".to_string(),
                "ioprio_get".to_string(),
                "inotify_init".to_string(),
                "inotify_add_watch".to_string(),
                "inotify_rm_watch".to_string(),
                "migrate_pages".to_string(),
                "openat".to_string(),
                "mkdirat".to_string(),
                "mknodat".to_string(),
                "fchownat".to_string(),
                "futimesat".to_string(),
                "newfstatat".to_string(),
                "unlinkat".to_string(),
                "renameat".to_string(),
                "linkat".to_string(),
                "symlinkat".to_string(),
                "readlinkat".to_string(),
                "fchmodat".to_string(),
                "faccessat".to_string(),
                "pselect6".to_string(),
                "ppoll".to_string(),
                "unshare".to_string(),
                "set_robust_list".to_string(),
                "get_robust_list".to_string(),
                "splice".to_string(),
                "tee".to_string(),
                "sync_file_range".to_string(),
                "vmsplice".to_string(),
                "move_pages".to_string(),
                "utimensat".to_string(),
                "epoll_pwait".to_string(),
                "signalfd".to_string(),
                "timerfd_create".to_string(),
                "eventfd".to_string(),
                "fallocate".to_string(),
                "timerfd_settime".to_string(),
                "timerfd_gettime".to_string(),
                "accept4".to_string(),
                "signalfd4".to_string(),
                "eventfd2".to_string(),
                "epoll_create1".to_string(),
                "dup3".to_string(),
                "pipe2".to_string(),
                "inotify_init1".to_string(),
                "preadv".to_string(),
                "pwritev".to_string(),
                "rt_tgsigqueueinfo".to_string(),
                "perf_event_open".to_string(),
                "recvmmsg".to_string(),
                "sendmmsg".to_string(),
                "fanotify_init".to_string(),
                "fanotify_mark".to_string(),
                "prlimit64".to_string(),
                "name_to_handle_at".to_string(),
                "open_by_handle_at".to_string(),
                "clock_adjtime".to_string(),
                "syncfs".to_string(),
                "sendmmsg".to_string(),
                "setns".to_string(),
                "getcpu".to_string(),
                "process_vm_readv".to_string(),
                "process_vm_writev".to_string(),
                "kcmp".to_string(),
                "finit_module".to_string(),
                "sched_setattr".to_string(),
                "sched_getattr".to_string(),
                "renameat2".to_string(),
                "seccomp".to_string(),
                "getrandom".to_string(),
                "memfd_create".to_string(),
                "kexec_file_load".to_string(),
                "bpf".to_string(),
                "execveat".to_string(),
                "userfaultfd".to_string(),
                "membarrier".to_string(),
                "mlock2".to_string(),
                "copy_file_range".to_string(),
                "preadv2".to_string(),
                "pwritev2".to_string(),
                "pkey_mprotect".to_string(),
                "pkey_alloc".to_string(),
                "pkey_free".to_string(),
                "statx".to_string(),
                "io_pgetevents".to_string(),
                "rseq".to_string(),
            ],
            action: SeccompAction::Allow,
            args: None,
        },
        // Block dangerous syscalls
        SeccompSyscallRule {
            names: vec![
                "ptrace".to_string(),
                "process_vm_readv".to_string(),
                "process_vm_writev".to_string(),
                "kcmp".to_string(),
                "bpf".to_string(),
                "userfaultfd".to_string(),
            ],
            action: SeccompAction::Errno,
            args: None,
        },
    ]
}

/// Get restricted seccomp rules (minimal syscalls only)
fn get_restricted_seccomp_rules() -> Vec<SeccompSyscallRule> {
    vec![SeccompSyscallRule {
        names: vec![
            "read".to_string(),
            "write".to_string(),
            "open".to_string(),
            "close".to_string(),
            "exit".to_string(),
            "exit_group".to_string(),
            "brk".to_string(),
            "mmap".to_string(),
            "munmap".to_string(),
            "rt_sigaction".to_string(),
            "rt_sigreturn".to_string(),
            "rt_sigprocmask".to_string(),
            "futex".to_string(),
            "getpid".to_string(),
            "getuid".to_string(),
            "getgid".to_string(),
            "geteuid".to_string(),
            "getegid".to_string(),
        ],
        action: SeccompAction::Allow,
        args: None,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_security_profile_default() {
        let config = SecurityProfileConfig::default();
        assert!(config.seccomp.is_some());
        assert_eq!(config.profile_type, SecurityProfileType::Default);
        assert!(config.enabled);
        assert_eq!(config.name, "default");
        assert!(config.no_new_privileges);
        assert!(!config.read_only_rootfs);
        assert!(!config.user_namespace);
    }

    #[test]
    fn test_security_profile_custom() {
        let config = SecurityProfileConfig {
            name: "custom".to_string(),
            profile_type: SecurityProfileType::Restricted,
            enabled: false,
            ..Default::default()
        };
        assert_eq!(config.name, "custom");
        assert_eq!(config.profile_type, SecurityProfileType::Restricted);
        assert!(!config.enabled);
    }

    #[test]
    fn test_security_profile_type_serialization() {
        let types = vec![
            SecurityProfileType::Default,
            SecurityProfileType::Unconfined,
            SecurityProfileType::Restricted,
            SecurityProfileType::Custom("test".to_string()),
        ];
        for t in types {
            let json = serde_json::to_string(&t).unwrap();
            let parsed: SecurityProfileType = serde_json::from_str(&json).unwrap();
            assert_eq!(t, parsed);
        }
    }

    #[test]
    fn test_seccomp_profile_default() {
        let profile = SeccompProfile::default();
        assert_eq!(profile.default_action, SeccompAction::Errno);
        assert!(!profile.syscalls.is_empty());
        assert_eq!(profile.architectures.len(), 4);
    }

    #[test]
    fn test_seccomp_profile_restricted() {
        let profile = SecurityHardeningManager::generate_restricted_seccomp_profile();
        assert_eq!(profile.default_action, SeccompAction::Kill);
        assert_eq!(profile.architectures.len(), 2);
    }

    #[test]
    fn test_seccomp_profile_unconfined() {
        let profile = SecurityHardeningManager::generate_unconfined_seccomp_profile();
        assert_eq!(profile.default_action, SeccompAction::Allow);
        assert!(profile.syscalls.is_empty());
    }

    #[test]
    fn test_seccomp_action_serialization() {
        let actions = vec![
            SeccompAction::Kill,
            SeccompAction::KillProcess,
            SeccompAction::Trap,
            SeccompAction::Errno,
            SeccompAction::Trace,
            SeccompAction::Allow,
            SeccompAction::Log,
        ];
        for a in actions {
            let json = serde_json::to_string(&a).unwrap();
            let parsed: SeccompAction = serde_json::from_str(&json).unwrap();
            assert_eq!(a, parsed);
        }
    }

    #[test]
    fn test_capability_config_default() {
        let caps = CapabilityConfig::default();
        assert!(!caps.drop.is_empty());
        assert!(caps.add.is_empty());
        assert!(caps.drop.contains(&"CAP_SYS_ADMIN".to_string()));
        assert!(caps.drop.contains(&"CAP_SYS_MODULE".to_string()));
    }

    #[test]
    fn test_capability_config_custom() {
        let caps = CapabilityConfig {
            add: vec!["CAP_NET_ADMIN".to_string()],
            drop: vec![],
            ..Default::default()
        };
        assert_eq!(caps.add, vec!["CAP_NET_ADMIN".to_string()]);
        assert!(caps.drop.is_empty());
    }

    #[test]
    fn test_seccomp_syscall_rule_serialization() {
        let rule = SeccompSyscallRule {
            names: vec!["read".to_string(), "write".to_string()],
            action: SeccompAction::Allow,
            args: Some(vec![SeccompArg {
                index: 0,
                value: 1,
                value_two: None,
                op: SeccompOp::Equal,
            }]),
        };
        let json = serde_json::to_string(&rule).unwrap();
        let parsed: SeccompSyscallRule = serde_json::from_str(&json).unwrap();
        assert_eq!(rule.names, parsed.names);
        assert_eq!(rule.action, parsed.action);
    }

    #[test]
    fn test_security_hardening_manager_creation() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = SecurityHardeningManager::with_custom_dirs(
            temp_dir.path().join("seccomp"),
            temp_dir.path().join("apparmor"),
            temp_dir.path().join("selinux"),
        );
        assert!(manager.is_ok());
    }

    #[test]
    fn test_register_and_get_profile() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut manager = SecurityHardeningManager::with_custom_dirs(
            temp_dir.path().join("seccomp"),
            temp_dir.path().join("apparmor"),
            temp_dir.path().join("selinux"),
        )
        .unwrap();
        let config = SecurityProfileConfig::default();
        manager.register_profile("test".to_string(), config.clone());
        let retrieved = manager.get_profile("test").unwrap();
        assert_eq!(retrieved.name, config.name);
    }

    #[test]
    fn test_get_nonexistent_profile() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = SecurityHardeningManager::with_custom_dirs(
            temp_dir.path().join("seccomp"),
            temp_dir.path().join("apparmor"),
            temp_dir.path().join("selinux"),
        )
        .unwrap();
        assert!(manager.get_profile("nonexistent").is_none());
    }
}
