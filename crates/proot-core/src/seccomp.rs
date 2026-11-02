//! Seccomp filter for PRoot

use anyhow::Result;

/// Seccomp filter for PRoot
pub struct SeccompFilter {
    enabled: bool,
    filter_program: Vec<u8>,
    allowed_syscalls: Vec<i64>,
    denied_syscalls: Vec<i64>,
}

impl SeccompFilter {
    pub fn new(config: &crate::cli::ProotConfig) -> Result<Self> {
        let mut filter = Self {
            enabled: config.seccomp,
            filter_program: Vec::new(),
            allowed_syscalls: Vec::new(),
            denied_syscalls: Vec::new(),
        };

        // Add Android-specific allowed syscalls
        filter.add_android_allowed_syscalls();

        Ok(filter)
    }

    fn add_android_allowed_syscalls(&mut self) {
        // Android-specific syscalls that should be allowed
        self.allowed_syscalls.extend([
            336, // rseq
            324, // membarrier
            347, // process_vm_readv
            348, // process_vm_writev
            435, // clone3
            437, // openat2
            151, // mlock
            152, // munlock
            439, // faccessat2
            291, // statx
            434, // pidfd_open
            438, // pidfd_getfd
            440, // process_madvise
            441, // epoll_pwait2
            442, // mount_setattr
            443, // quotactl_fd
            444, // landlock_create_ruleset
            445, // landlock_add_rule
            446, // landlock_restrict_self
            447, // memfd_secret
            448, // process_mrelease
            449, // futex_waitv
            450, // set_mempolicy_home_node
            451, // cachedctl
            452, // sched_setattr
            453, // move_mount
            454, // fsopen
            455, // fsconfig
            456, // fsmount
            457, // fspick
            458, // move_pages
        ]);
    }

    pub fn install(&self) -> anyhow::Result<()> {
        if !self.enabled {
            return Ok(());
        }

        unsafe {
            // Set no_new_privs
            if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
                return Err(anyhow::anyhow!("prctl(PR_SET_NO_NEW_PRIVS) failed"));
            }
        }

        // In a full implementation, we would build and install a BPF filter
        // For now, we just document the intent

        Ok(())
    }

    pub fn add_allowed_syscall(&mut self, syscall: i64) {
        self.allowed_syscalls.push(syscall);
    }

    pub fn add_denied_syscall(&mut self, syscall: i64) {
        self.denied_syscalls.push(syscall);
    }
}
