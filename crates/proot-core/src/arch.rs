//! Architecture definitions for PRoot

use std::fmt;

/// Supported architectures
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Arch {
    X86_64,
    X86,
    Arm64,
    Arm,
    Riscv64,
}

impl Arch {
    pub fn from_host() -> Self {
        match std::env::consts::ARCH {
            "x86_64" => Arch::X86_64,
            "x86" => Arch::X86,
            "aarch64" => Arch::Arm64,
            "arm" => Arch::Arm,
            "riscv64" => Arch::Riscv64,
            _ => panic!("Unsupported architecture: {}", std::env::consts::ARCH),
        }
    }

    pub fn to_str(&self) -> &'static str {
        match self {
            Arch::X86_64 => "x86_64",
            Arch::X86 => "x86",
            Arch::Arm64 => "aarch64",
            Arch::Arm => "arm",
            Arch::Riscv64 => "riscv64",
        }
    }

    pub fn uname_machine(&self) -> &'static str {
        match self {
            Arch::X86_64 => "x86_64",
            Arch::X86 => "i686",
            Arch::Arm64 => "aarch64",
            Arch::Arm => "armv7l",
            Arch::Riscv64 => "riscv64",
        }
    }

    pub fn pointer_size(&self) -> usize {
        match self {
            Arch::X86_64 | Arch::Arm64 | Arch::Riscv64 => 8,
            Arch::X86 | Arch::Arm => 4,
        }
    }

    pub fn is_64bit(&self) -> bool {
        self.pointer_size() == 8
    }

    pub fn supports_32bit(&self) -> bool {
        matches!(self, Arch::X86_64 | Arch::Arm64)
    }
}

impl fmt::Display for Arch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_str())
    }
}

/// Get the host architecture
pub fn get_arch() -> Arch {
    Arch::from_host()
}

/// Syscall number translation
pub mod sysnum {
    use libc::c_long;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Sysnum(pub c_long);

    impl Sysnum {
        pub const fn new(n: c_long) -> Self {
            Sysnum(n)
        }
    }

    // Common syscalls (aarch64)
    pub const SYS_READ: c_long = 63;
    pub const SYS_WRITE: c_long = 64;
    pub const SYS_OPEN: c_long = 56;
    pub const SYS_CLOSE: c_long = 57;
    pub const SYS_STAT: c_long = 79;
    pub const SYS_FSTAT: c_long = 80;
    pub const SYS_LSTAT: c_long = 81;
    pub const SYS_POLL: c_long = 86;
    pub const SYS_LSEEK: c_long = 62;
    pub const SYS_MMAP: c_long = 222;
    pub const SYS_MPROTECT: c_long = 226;
    pub const SYS_MUNMAP: c_long = 215;
    pub const SYS_BRK: c_long = 214;
    pub const SYS_RT_SIGACTION: c_long = 134;
    pub const SYS_RT_SIGPROCMASK: c_long = 135;
    pub const SYS_IOCTL: c_long = 29;
    pub const SYS_PREAD64: c_long = 67;
    pub const SYS_PWRITE64: c_long = 68;
    pub const SYS_READV: c_long = 65;
    pub const SYS_WRITEV: c_long = 66;
    pub const SYS_ACCESS: c_long = 48;
    pub const SYS_PIPE: c_long = 42;
    pub const SYS_SELECT: c_long = 91;
    pub const SYS_SCHED_YIELD: c_long = 124;
    pub const SYS_MREMAP: c_long = 216;
    pub const SYS_MSYNC: c_long = 227;
    pub const SYS_MINCORE: c_long = 228;
    pub const SYS_MADVISE: c_long = 229;
    pub const SYS_SHMGET: c_long = 241;
    pub const SYS_SHMAT: c_long = 242;
    pub const SYS_SHMCTL: c_long = 243;
    pub const SYS_DUP: c_long = 23;
    pub const SYS_DUP2: c_long = 24;
    pub const SYS_PAUSE: c_long = 101;
    pub const SYS_NANOSLEEP: c_long = 101;
    pub const SYS_GETITIMER: c_long = 102;
    pub const SYS_ALARM: c_long = 103;
    pub const SYS_SETITIMER: c_long = 104;
    pub const SYS_GETPID: c_long = 172;
    pub const SYS_SENDFILE: c_long = 70;
    pub const SYS_SOCKET: c_long = 198;
    pub const SYS_CONNECT: c_long = 203;
    pub const SYS_ACCEPT: c_long = 202;
    pub const SYS_SENDTO: c_long = 206;
    pub const SYS_RECVFROM: c_long = 207;
    pub const SYS_SENDMSG: c_long = 209;
    pub const SYS_RECVMSG: c_long = 210;
    pub const SYS_SHUTDOWN: c_long = 210;
    pub const SYS_BIND: c_long = 200;
    pub const SYS_LISTEN: c_long = 201;
    pub const SYS_GETSOCKNAME: c_long = 204;
    pub const SYS_GETPEERNAME: c_long = 205;
    pub const SYS_SOCKETPAIR: c_long = 199;
    pub const SYS_SETSOCKOPT: c_long = 208;
    pub const SYS_GETSOCKOPT: c_long = 207;
    pub const SYS_CLONE: c_long = 120;
    pub const SYS_FORK: c_long = 121;
    pub const SYS_VFORK: c_long = 122;
    pub const SYS_EXECVE: c_long = 221;
    pub const SYS_EXIT: c_long = 93;
    pub const SYS_WAIT4: c_long = 114;
    pub const SYS_KILL: c_long = 129;
    pub const SYS_UNAME: c_long = 160;
    pub const SYS_GETUID: c_long = 174;
    pub const SYS_GETGID: c_long = 176;
    pub const SYS_GETEUID: c_long = 175;
    pub const SYS_GETEGID: c_long = 177;
    pub const SYS_SETUID: c_long = 146;
    pub const SYS_SETGID: c_long = 147;
    pub const SYS_GETPPID: c_long = 173;
    pub const SYS_GETPGRP: c_long = 171;
    pub const SYS_SETSID: c_long = 170;
    pub const SYS_GETPGID: c_long = 178;
    pub const SYS_SETGROUPS: c_long = 150;
    pub const SYS_GETGROUPS: c_long = 151;
    pub const SYS_GETRLIMIT: c_long = 163;
    pub const SYS_SETRLIMIT: c_long = 164;
    pub const SYS_GETRUSAGE: c_long = 165;
    pub const SYS_GETTIMEOFDAY: c_long = 169;
    pub const SYS_SETTIMEOFDAY: c_long = 170;

    // Android-specific syscalls
    pub const SYS_RSEQ: c_long = 336;
    pub const SYS_MEMBARRIER: c_long = 324;
    pub const SYS_PROCESS_VM_READV: c_long = 347;
    pub const SYS_PROCESS_VM_WRITEV: c_long = 348;
    pub const SYS_CLONE3: c_long = 435;
    pub const SYS_OPENAT2: c_long = 437;
    pub const SYS_MLOCK: c_long = 151;
    pub const SYS_MUNLOCK: c_long = 152;
    pub const SYS_ARCH_PRCTL: c_long = 227;
    pub const SYS_PRLIMIT64: c_long = 302;
    pub const SYS_FACCESSAT2: c_long = 439;
    pub const SYS_STATX: c_long = 291;
    pub const SYS_IO_PGETEVENTS: c_long = 301;
    pub const SYS_PIDFD_SEND_SIGNAL: c_long = 424;
    pub const SYS_PIDFD_OPEN: c_long = 434;
    pub const SYS_PIDFD_GETFD: c_long = 438;
    pub const SYS_PROCESS_MADVISE: c_long = 440;
    pub const SYS_EPOLL_PWAIT2: c_long = 441;
    pub const SYS_MOUNT_SETATTR: c_long = 442;
    pub const SYS_QUOTACTL_FD: c_long = 443;
    pub const SYS_LANDLOCK_CREATE_RULESET: c_long = 444;
    pub const SYS_LANDLOCK_ADD_RULE: c_long = 445;
    pub const SYS_LANDLOCK_RESTRICT_SELF: c_long = 446;
    pub const SYS_MEMFD_SECRET: c_long = 447;
    pub const SYS_PROCESS_MRELEASE: c_long = 448;
    pub const SYS_FUTEX_WAITV: c_long = 449;
    pub const SYS_SET_MEMPOLICY_HOME_NODE: c_long = 450;
    pub const SYS_CACHEDLCTL: c_long = 451;
    pub const SYS_SCHED_SETATTR: c_long = 452;
    pub const SYS_MOVE_MOUNT: c_long = 453;
    pub const SYS_FSOPEN: c_long = 454;
    pub const SYS_FSCONFIG: c_long = 455;
    pub const SYS_FSMOUNT: c_long = 456;
    pub const SYS_FSPICK: c_long = 457;
    pub const SYS_MOVE_PAGES: c_long = 458;
}
