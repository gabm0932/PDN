//! System call handler for PRoot

use anyhow::Result;
use std::collections::HashMap;

use crate::tracee::{PathInfo, SyscallInfo, TraceeState};
use crate::tracee_state::TraceeStateManager;
use crate::vfs::VfsManager;

/// Trait for system call handler functions
pub trait SyscallHandlerFn:
    Fn(
        &mut TraceeState,
        &mut SyscallInfo,
        &mut PathInfo,
        &mut TraceeStateManager,
        &mut VfsManager,
    ) -> anyhow::Result<()>
    + Send
    + Sync
{
}

impl<T> SyscallHandlerFn for T where
    T: Fn(
            &mut TraceeState,
            &mut SyscallInfo,
            &mut PathInfo,
            &mut TraceeStateManager,
            &mut VfsManager,
        ) -> anyhow::Result<()>
        + Send
        + Sync
{
}

/// System call handler
pub struct SyscallHandler {
    handlers: HashMap<i64, Box<dyn SyscallHandlerFn>>,
    config: SyscallConfig,
}

#[derive(Debug, Clone)]
pub struct SyscallConfig {
    pub trace_all: bool,
    pub trace_paths: bool,
    pub allow_android_syscalls: bool,
}

impl Default for SyscallConfig {
    fn default() -> Self {
        Self {
            trace_all: false,
            trace_paths: true,
            allow_android_syscalls: true,
        }
    }
}

impl SyscallHandler {
    pub fn new(config: SyscallConfig) -> Result<Self> {
        let mut handlers: HashMap<i64, Box<dyn SyscallHandlerFn>> = HashMap::new();

        // Register path-related syscalls
        Self::register_path_syscalls(&mut handlers);

        // Register file operation syscalls
        Self::register_file_syscalls(&mut handlers);

        // Register process syscalls
        Self::register_process_syscalls(&mut handlers);

        // Register Android-specific syscalls
        Self::register_android_syscalls(&mut handlers);

        Ok(Self { handlers, config })
    }

    fn register_path_syscalls(handlers: &mut HashMap<i64, Box<dyn SyscallHandlerFn>>) {
        // open, openat, openat2
        handlers.insert(
            56,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // open
                Ok(())
            }),
        );

        handlers.insert(
            257,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // openat
                Ok(())
            }),
        );

        // openat2 - Android
        handlers.insert(
            437,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // openat2
                Ok(())
            }),
        );

        // mkdir, mkdirat
        handlers.insert(
            34,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // mkdir
                Ok(())
            }),
        );

        handlers.insert(
            258,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // mkdirat
                Ok(())
            }),
        );

        // rmdir
        handlers.insert(
            49,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // rmdir
                Ok(())
            }),
        );

        // unlink, unlinkat
        handlers.insert(
            263,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // unlinkat
                Ok(())
            }),
        );

        // rename, renameat, renameat2
        handlers.insert(
            38,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // rename
                Ok(())
            }),
        );

        handlers.insert(
            264,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // renameat
                Ok(())
            }),
        );

        handlers.insert(
            38,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // renameat2 (same as rename on aarch64)
                Ok(())
            }),
        );

        // link, linkat
        handlers.insert(
            37,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // link
                Ok(())
            }),
        );

        handlers.insert(
            265,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // linkat
                Ok(())
            }),
        );

        // symlink, symlinkat
        handlers.insert(
            88,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // symlink
                Ok(())
            }),
        );

        handlers.insert(
            89,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // symlinkat
                Ok(())
            }),
        );

        // readlink, readlinkat
        handlers.insert(
            89,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // readlink
                Ok(())
            }),
        );

        handlers.insert(
            89,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // readlinkat
                Ok(())
            }),
        );

        // chdir, fchdir
        handlers.insert(
            80,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // chdir
                Ok(())
            }),
        );

        handlers.insert(
            81,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // fchdir
                Ok(())
            }),
        );

        // getcwd
        handlers.insert(
            79,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // getcwd
                Ok(())
            }),
        );

        // chroot
        handlers.insert(
            61,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // chroot
                Ok(())
            }),
        );
    }

    fn register_file_syscalls(handlers: &mut HashMap<i64, Box<dyn SyscallHandlerFn>>) {
        // read, write
        handlers.insert(
            63,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // read
                Ok(())
            }),
        );

        handlers.insert(
            64,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // write
                Ok(())
            }),
        );

        // readv, writev
        handlers.insert(
            65,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // readv
                Ok(())
            }),
        );

        handlers.insert(
            66,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // writev
                Ok(())
            }),
        );

        // pread, pwrite
        handlers.insert(
            67,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // pread
                Ok(())
            }),
        );

        handlers.insert(
            68,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // pwrite
                Ok(())
            }),
        );

        // readv, writev
        handlers.insert(
            65,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // readv
                Ok(())
            }),
        );

        handlers.insert(
            66,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // writev
                Ok(())
            }),
        );

        // lseek
        handlers.insert(
            62,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // lseek
                Ok(())
            }),
        );

        // fstat, fstatat, stat, lstat, statx
        handlers.insert(
            80,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // fstat
                Ok(())
            }),
        );

        handlers.insert(
            79,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // stat
                Ok(())
            }),
        );

        handlers.insert(
            81,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // lstat
                Ok(())
            }),
        );

        handlers.insert(
            291,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // statx
                Ok(())
            }),
        );

        // chmod, fchmod, fchmodat
        handlers.insert(
            90,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // chmod
                Ok(())
            }),
        );

        handlers.insert(
            268,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // fchmodat
                Ok(())
            }),
        );

        // chown, fchown, fchownat, lchown
        handlers.insert(
            92,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // chown
                Ok(())
            }),
        );

        handlers.insert(
            260,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // fchownat
                Ok(())
            }),
        );

        handlers.insert(
            93,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // lchown
                Ok(())
            }),
        );

        // utime, utimensat, utimes
        handlers.insert(
            235,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // utimensat
                Ok(())
            }),
        );

        // truncate, ftruncate
        handlers.insert(
            76,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // truncate
                Ok(())
            }),
        );

        handlers.insert(
            77,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // ftruncate
                Ok(())
            }),
        );

        // fallocate
        handlers.insert(
            324,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // fallocate
                Ok(())
            }),
        );

        // sync, fsync, fdatasync
        handlers.insert(
            74,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // fsync
                Ok(())
            }),
        );

        handlers.insert(
            75,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // fdatasync
                Ok(())
            }),
        );

        // sync
        handlers.insert(
            36,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // sync
                Ok(())
            }),
        );

        // pipe, pipe2
        handlers.insert(
            59,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // pipe
                Ok(())
            }),
        );

        handlers.insert(
            293,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // pipe2
                Ok(())
            }),
        );

        // dup, dup2, dup3
        handlers.insert(
            23,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // dup
                Ok(())
            }),
        );

        handlers.insert(
            24,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // dup2
                Ok(())
            }),
        );

        handlers.insert(
            24,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // dup3 (same as dup2 on aarch64)
                Ok(())
            }),
        );

        // epoll
        handlers.insert(
            20,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // epoll_create
                Ok(())
            }),
        );

        handlers.insert(
            21,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // epoll_create1
                Ok(())
            }),
        );

        handlers.insert(
            23,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // epoll_ctl
                Ok(())
            }),
        );

        handlers.insert(
            24,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // epoll_pwait
                Ok(())
            }),
        );

        handlers.insert(
            441,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // epoll_pwait2
                Ok(())
            }),
        );

        // inotify
        handlers.insert(
            253,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // inotify_init
                Ok(())
            }),
        );

        handlers.insert(
            254,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // inotify_init1
                Ok(())
            }),
        );

        handlers.insert(
            255,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // inotify_add_watch
                Ok(())
            }),
        );

        handlers.insert(
            256,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // inotify_rm_watch
                Ok(())
            }),
        );

        // timerfd
        handlers.insert(
            283,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // timerfd_create
                Ok(())
            }),
        );

        handlers.insert(
            284,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // timerfd_settime
                Ok(())
            }),
        );

        handlers.insert(
            285,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // timerfd_gettime
                Ok(())
            }),
        );

        // eventfd
        handlers.insert(
            286,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // eventfd
                Ok(())
            }),
        );

        handlers.insert(
            287,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // eventfd2
                Ok(())
            }),
        );

        // signalfd
        handlers.insert(
            289,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // signalfd
                Ok(())
            }),
        );

        handlers.insert(
            290,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // signalfd4
                Ok(())
            }),
        );

        // fanotify
        handlers.insert(
            300,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // fanotify_init
                Ok(())
            }),
        );

        handlers.insert(
            301,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // fanotify_mark
                Ok(())
            }),
        );
    }

    fn register_process_syscalls(handlers: &mut HashMap<i64, Box<dyn SyscallHandlerFn>>) {
        // clone, clone3
        handlers.insert(
            120,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // clone
                Ok(())
            }),
        );

        handlers.insert(
            435,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // clone3
                Ok(())
            }),
        );

        // fork, vfork
        handlers.insert(
            121,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // fork
                Ok(())
            }),
        );

        handlers.insert(
            122,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // vfork
                Ok(())
            }),
        );

        // execve, execveat
        handlers.insert(
            221,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // execve
                Ok(())
            }),
        );

        handlers.insert(
            221,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // execveat (same number on aarch64)
                Ok(())
            }),
        );

        // exit, exit_group
        handlers.insert(
            93,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // exit
                Ok(())
            }),
        );

        handlers.insert(
            94,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // exit_group
                Ok(())
            }),
        );

        // wait, wait4, waitpid, waitid
        handlers.insert(
            114,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // wait4
                Ok(())
            }),
        );

        handlers.insert(
            114,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // waitid (same number on aarch64)
                Ok(())
            }),
        );

        // kill, tkill, tgkill
        handlers.insert(
            129,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // kill
                Ok(())
            }),
        );

        handlers.insert(
            129,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // tkill
                Ok(())
            }),
        );

        handlers.insert(
            129,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // tgkill
                Ok(())
            }),
        );

        // getpid, getppid, gettid
        handlers.insert(
            172,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // getpid
                Ok(())
            }),
        );

        handlers.insert(
            173,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // getppid
                Ok(())
            }),
        );

        handlers.insert(
            178,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // gettid
                Ok(())
            }),
        );

        // setuid, setgid, setgroups
        handlers.insert(
            146,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // setuid
                Ok(())
            }),
        );

        handlers.insert(
            147,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // setgid
                Ok(())
            }),
        );

        handlers.insert(
            150,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // setgroups
                Ok(())
            }),
        );

        // prlimit
        handlers.insert(
            302,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // prlimit64
                Ok(())
            }),
        );

        // setrlimit, getrlimit
        handlers.insert(
            163,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // getrlimit
                Ok(())
            }),
        );

        handlers.insert(
            164,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // setrlimit
                Ok(())
            }),
        );

        // sched
        handlers.insert(
            144,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // sched_setparam
                Ok(())
            }),
        );

        handlers.insert(
            145,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // sched_getparam
                Ok(())
            }),
        );

        handlers.insert(
            144,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // sched_setscheduler
                Ok(())
            }),
        );

        handlers.insert(
            145,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // sched_getscheduler
                Ok(())
            }),
        );

        handlers.insert(
            144,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // sched_yield
                Ok(())
            }),
        );

        handlers.insert(
            144,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // sched_get_priority_max
                Ok(())
            }),
        );

        handlers.insert(
            144,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // sched_get_priority_min
                Ok(())
            }),
        );

        handlers.insert(
            452,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // sched_setattr
                Ok(())
            }),
        );

        handlers.insert(
            452,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // sched_getattr
                Ok(())
            }),
        );

        // cpu affinity
        handlers.insert(
            203,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // sched_setaffinity
                Ok(())
            }),
        );

        handlers.insert(
            204,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // sched_getaffinity
                Ok(())
            }),
        );

        // ptrace
        handlers.insert(
            127,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // ptrace
                Ok(())
            }),
        );

        // seccomp
        handlers.insert(
            317,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // seccomp
                Ok(())
            }),
        );

        // perf_event
        handlers.insert(
            298,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // perf_event_open
                Ok(())
            }),
        );

        // rseq
        handlers.insert(
            336,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // rseq
                Ok(())
            }),
        );

        // membarrier
        handlers.insert(
            324,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // membarrier
                Ok(())
            }),
        );

        // process_vm_readv/writev
        handlers.insert(
            347,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // process_vm_readv
                Ok(())
            }),
        );

        handlers.insert(
            348,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // process_vm_writev
                Ok(())
            }),
        );

        // pidfd
        handlers.insert(
            424,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // pidfd_send_signal
                Ok(())
            }),
        );

        handlers.insert(
            434,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // pidfd_open
                Ok(())
            }),
        );

        handlers.insert(
            438,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // pidfd_getfd
                Ok(())
            }),
        );

        // landlock
        handlers.insert(
            444,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // landlock_create_ruleset
                Ok(())
            }),
        );

        handlers.insert(
            445,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // landlock_add_rule
                Ok(())
            }),
        );

        handlers.insert(
            446,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // landlock_restrict_self
                Ok(())
            }),
        );
    }

    fn register_android_syscalls(handlers: &mut HashMap<i64, Box<dyn SyscallHandlerFn>>) {
        // rseq
        handlers.insert(
            336,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // rseq
                Ok(())
            }),
        );

        // membarrier
        handlers.insert(
            324,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // membarrier
                Ok(())
            }),
        );

        // process_vm_readv/writev
        handlers.insert(
            347,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // process_vm_readv
                Ok(())
            }),
        );

        handlers.insert(
            348,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // process_vm_writev
                Ok(())
            }),
        );

        // clone3
        handlers.insert(
            435,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // clone3
                Ok(())
            }),
        );

        // openat2
        handlers.insert(
            437,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // openat2
                Ok(())
            }),
        );

        // mlock/munlock
        handlers.insert(
            151,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // mlock
                Ok(())
            }),
        );

        handlers.insert(
            152,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // munlock
                Ok(())
            }),
        );

        // faccessat2
        handlers.insert(
            439,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // faccessat2
                Ok(())
            }),
        );

        // statx
        handlers.insert(
            291,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // statx
                Ok(())
            }),
        );

        // io_pgetevents
        handlers.insert(
            301,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // io_pgetevents
                Ok(())
            }),
        );

        // pidfd_send_signal
        handlers.insert(
            424,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // pidfd_send_signal
                Ok(())
            }),
        );

        // pidfd_open
        handlers.insert(
            434,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // pidfd_open
                Ok(())
            }),
        );

        // clone3
        handlers.insert(
            435,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // clone3
                Ok(())
            }),
        );

        // openat2
        handlers.insert(
            437,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // openat2
                Ok(())
            }),
        );

        // pidfd_getfd
        handlers.insert(
            438,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // pidfd_getfd
                Ok(())
            }),
        );

        // faccessat2
        handlers.insert(
            439,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // faccessat2
                Ok(())
            }),
        );

        // process_madvise
        handlers.insert(
            440,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // process_madvise
                Ok(())
            }),
        );

        // epoll_pwait2
        handlers.insert(
            441,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // epoll_pwait2
                Ok(())
            }),
        );

        // mount_setattr
        handlers.insert(
            442,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // mount_setattr
                Ok(())
            }),
        );

        // quotactl_fd
        handlers.insert(
            443,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // quotactl_fd
                Ok(())
            }),
        );

        // landlock
        handlers.insert(
            444,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // landlock_create_ruleset
                Ok(())
            }),
        );

        handlers.insert(
            445,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // landlock_add_rule
                Ok(())
            }),
        );

        handlers.insert(
            446,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // landlock_restrict_self
                Ok(())
            }),
        );

        // memfd_secret
        handlers.insert(
            447,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // memfd_secret
                Ok(())
            }),
        );

        // process_mrelease
        handlers.insert(
            448,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // process_mrelease
                Ok(())
            }),
        );

        // futex_waitv
        handlers.insert(
            449,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // futex_waitv
                Ok(())
            }),
        );

        // set_mempolicy_home_node
        handlers.insert(
            450,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // set_mempolicy_home_node
                Ok(())
            }),
        );

        // cachedctl
        handlers.insert(
            451,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // cachedctl
                Ok(())
            }),
        );

        // sched_setattr
        handlers.insert(
            452,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // sched_setattr
                Ok(())
            }),
        );

        // move_mount
        handlers.insert(
            453,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // move_mount
                Ok(())
            }),
        );

        // fsopen
        handlers.insert(
            454,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // fsopen
                Ok(())
            }),
        );

        handlers.insert(
            455,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // fsconfig
                Ok(())
            }),
        );

        handlers.insert(
            456,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // fsmount
                Ok(())
            }),
        );

        handlers.insert(
            457,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // fspick
                Ok(())
            }),
        );

        handlers.insert(
            458,
            Box::new(|_state, _info, _path_info, _tracee_mgr, _vfs_mgr| {
                // move_pages
                Ok(())
            }),
        );
    }

    pub fn handle_entry(
        &mut self,
        state: &mut TraceeState,
        info: &mut SyscallInfo,
        path_info: &mut PathInfo,
        tracee_mgr: &mut TraceeStateManager,
        vfs_mgr: &mut VfsManager,
    ) -> Result<()> {
        if let Some(handler) = self.handlers.get(&info.number) {
            handler(state, info, path_info, tracee_mgr, vfs_mgr)?;
        }
        Ok(())
    }

    pub fn handle_exit(
        &mut self,
        state: &mut TraceeState,
        info: &mut SyscallInfo,
        path_info: &mut PathInfo,
        tracee_mgr: &mut TraceeStateManager,
        vfs_mgr: &mut VfsManager,
    ) -> Result<()> {
        if let Some(handler) = self.handlers.get(&info.number) {
            handler(state, info, path_info, tracee_mgr, vfs_mgr)?;
        }
        Ok(())
    }
}
